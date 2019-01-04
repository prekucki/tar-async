use super::flat;
use super::Error;
use bytes::Bytes;
use futures::prelude::*;
use futures::{stream, try_ready};
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::Mutex;

struct DeepTarStreamInner<S> {
    upstream: S,
    position: u64,
    bytes: u64,
}

struct DeepTarStream<S> {
    inner: Arc<Mutex<DeepTarStreamInner<S>>>,
}

pub struct Entry<S: Stream<Item = flat::TarItem>>
where
    S::Error: Sync + Send + Debug + 'static,
{
    header: flat::TarEntry,
    position: u64,
    inner: Arc<Mutex<DeepTarStreamInner<S>>>,
}

impl<E: Sync + Send + Debug + 'static, S: Stream<Item = flat::TarItem, Error = Error<E>>> Entry<S> {
    #[inline]
    pub fn header(&self) -> &flat::TarEntry {
        &self.header
    }
}

impl<E: Sync + Send + Debug + 'static, S: Stream<Item = flat::TarItem, Error = Error<E>>> Stream
    for Entry<S>
{
    type Item = Bytes;
    type Error = Error<E>;

    fn poll(&mut self) -> Result<Async<Option<<Self as Stream>::Item>>, <Self as Stream>::Error> {
        self.inner.lock().unwrap().poll_entry_data(self.position)
    }
}

impl<S: Stream<Item = flat::TarItem>> Drop for Entry<S>
where
    S::Error: Sync + Send + Debug + 'static,
{
    fn drop(&mut self) {
        self.inner.lock().unwrap().bytes = 0;
    }
}

impl<E, S: Stream<Item = flat::TarItem, Error = Error<E>>> DeepTarStreamInner<S>
where
    E: Sync + Send + Debug + 'static,
{
    fn new(upstream: S) -> Self {
        DeepTarStreamInner {
            upstream,
            position: 0,
            bytes: 0,
        }
    }

    fn poll_entry_data(&mut self, position: u64) -> Result<Async<Option<Bytes>>, S::Error> {
        if self.position == position && self.bytes > 0 {
            match try_ready!(self.upstream.poll()) {
                Some(flat::TarItem::Chunk(bytes)) => {
                    self.bytes -= bytes.len() as u64;
                    Ok(Async::Ready(Some(bytes)))
                }
                None => Err(Error::UnexpectedEof),
                _ => panic!("read after end of file"),
            }
        } else {
            Ok(Async::Ready(None))
        }
    }

    fn poll_next_entry(&mut self) -> Result<Async<Option<(flat::TarEntry, u64)>>, S::Error> {
        if self.bytes > 0 {
            return Ok(Async::NotReady);
        }

        loop {
            match try_ready!(self.upstream.poll()) {
                Some(flat::TarItem::Entry(entry)) => {
                    self.position += 1;
                    self.bytes = entry.size();
                    return Ok(Async::Ready(Some((entry, self.position))));
                }
                None => return Ok(Async::Ready(None)),
                // Skip bytes
                _ => (),
            }
        }
    }
}

impl<E: Sync + Send + Debug + 'static, S: Stream<Item = flat::TarItem, Error = Error<E>>> Stream
    for DeepTarStream<S>
{
    type Item = Entry<S>;
    type Error = Error<E>;

    fn poll(&mut self) -> Result<Async<Option<<Self as Stream>::Item>>, <Self as Stream>::Error> {
        let mut inner = self.inner.lock().unwrap();

        match inner.poll_next_entry() {
            Err(e) => Err(e),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Ok(Async::Ready(Some((header, position)))) => Ok(Async::Ready(Some(Entry {
                header,
                position,
                inner: self.inner.clone(),
            }))),
        }
    }
}

pub fn decode_tar<TarStream: Stream<Item = Bytes>>(
    upstream: TarStream,
) -> impl Stream<
    Item = Entry<impl Stream<Item = flat::TarItem, Error = Error<TarStream::Error>>>,
    Error = Error<TarStream::Error>,
>
where
    TarStream::Error: std::fmt::Debug + Sync + Send + 'static,
{
    DeepTarStream {
        inner: Arc::new(Mutex::new(DeepTarStreamInner::new(flat::decode_tar(
            upstream,
        )))),
    }
}
