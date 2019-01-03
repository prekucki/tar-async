use super::Config;
use super::Error;
use bytes::{Buf, BufMut, Bytes, BytesMut, IntoBuf};
use failure::Fail;
use futures::prelude::*;
use futures::try_ready;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use tar::PaxExtensions;

struct RawEntryStreamInner<Upstream> {
    upstream: Upstream,
    buffer: BytesMut,
    tail: Option<Bytes>,
    in_entry: u64,
    locked: bool,
    config: Config,
}

const HEADER_SIZE: usize = 512;

impl<Upstream: Stream<Item = Bytes>> RawEntryStreamInner<Upstream>
where
    Upstream::Error: std::fmt::Debug + Sync + Send + 'static,
{
    fn new(upstream: Upstream) -> Self {
        RawEntryStreamInner {
            upstream,
            buffer: BytesMut::with_capacity(HEADER_SIZE),
            tail: None,
            config: Config {
                unpack_xattrs: false,
                preserve_permissions: false,
                preserve_mtime: false,
                ignore_zeros: false,
            },
            in_entry: 0,
            locked: false,
        }
    }

    fn fetch_entry_bytes(&mut self) -> Result<Async<Option<Bytes>>, Error<Upstream::Error>> {
        if let Some(mut tail) = self.tail.take() {
            if tail.len() as u64 <= self.in_entry {
                self.in_entry -= tail.len() as u64;
                Ok(Async::Ready(Some(tail)))
            } else {
                assert!(self.in_entry < std::usize::MAX as u64);
                eprintln!("tail={} / in_entry={}", tail.len(), self.in_entry);
                let mut chunk = tail.split_to(self.in_entry as usize);
                self.in_entry = 0;
                eprintln!(
                    "tail={} / in_entry={}, chunk={}",
                    tail.len(),
                    self.in_entry,
                    chunk.len()
                );
                self.tail = Some(tail);
                Ok(Async::Ready(Some(chunk)))
            }
        } else {
            if let Some(mut bytes) = try_ready!(self.upstream.poll()) {
                if bytes.len() as u64 <= self.in_entry {
                    self.in_entry -= bytes.len() as u64;
                    Ok(Async::Ready(Some(bytes)))
                } else {
                    eprintln!("bytes={} / in_entry={}", bytes.len(), self.in_entry);
                    let head = bytes.split_to(self.in_entry as usize);
                    self.in_entry = 0;
                    eprintln!(
                        "bytes={} / in_entry={}, head={}",
                        bytes.len(),
                        self.in_entry,
                        head.len()
                    );
                    self.tail = Some(bytes);
                    Ok(Async::Ready(Some(head)))
                }
            } else {
                if self.in_entry == 0 {
                    Ok(Async::Ready(None))
                } else {
                    Err(Error::UnexpectedEof)
                }
            }
        }
    }

    fn fetch_header(&mut self) -> Result<Async<Option<tar::Header>>, Error<Upstream::Error>> {
        assert_eq!(self.in_entry, 0);
        let mut header = tar::Header::new_old();
        loop {
            if let Some(mut tail) = self.tail.take() {
                if (tail.len() + self.buffer.len()) >= HEADER_SIZE {
                    if self.buffer.is_empty() {
                        Buf::copy_to_slice(
                            &mut tail.split_to(HEADER_SIZE).into_buf(),
                            header.as_mut_bytes(),
                        );
                        self.tail = Some(tail);
                        return Ok(Async::Ready(Some(header)));
                    } else {
                        eprintln!("tail={}, buffer={}", tail.len(), self.buffer.len());

                        let output_buf = self.buffer.as_ref();
                        let header_bytes = header.as_mut_bytes();
                        header_bytes[..output_buf.len()].copy_from_slice(output_buf);

                        let mut rem = tail
                            .split_to(header_bytes.len() - output_buf.len())
                            .into_buf();
                        rem.copy_to_slice(&mut header_bytes[output_buf.len()..]);
                        self.buffer.clear();
                        eprintln!(
                            "tail={}, buffer={}, header={}",
                            tail.len(),
                            self.buffer.len(),
                            512
                        );
                        self.tail = Some(tail);
                        return Ok(Async::Ready(Some(header)));
                    }
                } else {
                    eprintln!("BLS: {}", self.buffer.len());
                    self.buffer.put(tail);
                    eprintln!("BLE: {}", self.buffer.len());
                }
            }
            assert!(self.tail.is_none());
            if let Some(bytes) = try_ready!(self.upstream.poll()) {
                self.tail = Some(bytes)
            } else {
                if self.buffer.is_empty() {
                    return Ok(Async::Ready(None));
                } else {
                    return Err(Error::UnexpectedEof);
                }
            }
        }
    }
}

pub struct RawEntryStream<Upstream> {
    inner: Arc<Mutex<RawEntryStreamInner<Upstream>>>,
}

impl<Upstream: Stream<Item = Bytes>> Stream for RawEntryStream<Upstream>
where
    Upstream::Error: std::fmt::Debug + Sync + Send + 'static,
{
    type Item = RawAsyncEntry<Upstream>;
    type Error = Error<Upstream::Error>;

    fn poll(&mut self) -> Result<Async<Option<<Self as Stream>::Item>>, <Self as Stream>::Error> {
        let mut inner_ref = self.inner.lock().unwrap();

        if inner_ref.locked {
            return Ok(Async::NotReady);
        }

        while inner_ref.in_entry > 0 {
            let bytes = try_ready!(inner_ref.fetch_entry_bytes());
            println!("got {} bytes", bytes.unwrap().len());
        }

        if let Some(header) = try_ready!(inner_ref.fetch_header()) {
            let size = match header.size() {
                Ok(size) => size,
                Err(e) => return Err(Error::IoError(e)),
            };

            inner_ref.in_entry = (size + 511) & !(512 - 1);
            inner_ref.locked = true;
            Ok(Async::Ready(Some(RawAsyncEntry {
                master: self.inner.clone(),
                header,
                real_bytes: size,
            })))
        } else {
            Ok(Async::Ready(None))
        }
    }
}

pub struct RawAsyncEntry<Upstream> {
    master: Arc<Mutex<RawEntryStreamInner<Upstream>>>,
    header: tar::Header,
    real_bytes: u64,
}

impl<Upstream: Stream<Item = Bytes>> Stream for RawAsyncEntry<Upstream>
where
    Upstream::Error: std::fmt::Debug + Sync + Send + 'static,
{
    type Item = Bytes;
    type Error = Error<Upstream::Error>;

    fn poll(&mut self) -> Result<Async<Option<<Self as Stream>::Item>>, <Self as Stream>::Error> {
        if self.real_bytes == 0 {
            Ok(Async::Ready(None))
        } else {
            let to_read = self.real_bytes;
            if let Some(bytes) = try_ready!(self.master.lock().unwrap().fetch_entry_bytes()) {
                if bytes.len() as u64 > to_read {
                    self.real_bytes = 0;
                    Ok(Async::Ready(Some(bytes.slice_to(to_read as usize))))
                } else {
                    self.real_bytes -= bytes.len() as u64;
                    Ok(Async::Ready(Some(bytes)))
                }
            } else {
                if self.real_bytes == 0 {
                    Ok(Async::Ready(None))
                } else {
                    Err(Error::UnexpectedEof)
                }
            }
        }
    }
}

impl<Upstream> Drop for RawAsyncEntry<Upstream> {
    fn drop(&mut self) {
        self.master.lock().unwrap().locked = false;
    }
}

impl<Upstream> RawAsyncEntry<Upstream> {
    pub fn header(&self) -> &tar::Header {
        &self.header
    }
}

pub fn decode_tar<TarStream: Stream<Item = Bytes>>(
    upstream: TarStream,
) -> impl Stream<Item = RawAsyncEntry<TarStream>, Error = Error<TarStream::Error>>
where
    TarStream::Error: std::fmt::Debug + Sync + Send + 'static,
{
    RawEntryStream {
        inner: Arc::new(Mutex::new(RawEntryStreamInner::new(upstream))),
    }
}
