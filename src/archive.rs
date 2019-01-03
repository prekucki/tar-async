use super::raw::{self, RawTarItem};
use super::Error;
use bytes::{BufMut, Bytes, BytesMut};
use futures::{prelude::*, try_ready};
use std::fmt::{self, Debug, Formatter};
use std::path::Path;
use std::{io, str};

fn bytes2path(bytes: &[u8]) -> io::Result<&Path> {
    let s = str::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    Ok(Path::new(s))
}

pub struct TarEntry {
    path_bytes: Vec<u8>,
    link_bytes: Option<Vec<u8>>,
    pax_extensions: Option<Vec<u8>>,
    size: u64,
}

impl TarEntry {
    pub fn path(&self) -> io::Result<&Path> {
        bytes2path(self.path_bytes.as_slice())
    }

    pub fn link(&self) -> io::Result<Option<&Path>> {
        Ok(match self.link_bytes.as_ref() {
            Some(bytes) => Some(bytes2path(bytes)?),
            None => None,
        })
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}

#[derive(Debug)]
pub enum TarItem {
    Entry(TarEntry),
    Chunk(Bytes),
}

impl Debug for TarEntry {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "path={:?}, link={:?}, size={:?}",
            self.path(),
            self.link(),
            self.size()
        )
    }
}

struct EntryStream<U> {
    upstream: U,
    buffer: Option<BytesMut>,
    gnu_long_name: Option<Vec<u8>>,
    gnu_long_link: Option<Vec<u8>>,
    pax_extensions: Option<Vec<u8>>,
    state: State,
    processed: u32,
}

#[derive(Debug, PartialOrd, PartialEq)]
enum State {
    Clean,
    InGnuLongName,
    InGnuLongLink,
    InPaxExtensions,
}

impl<E: Debug + Send + Sync + 'static, U: Stream<Item = RawTarItem, Error = Error<E>>> Stream
    for EntryStream<U>
{
    type Item = TarItem;
    type Error = Error<E>;

    fn poll(&mut self) -> Result<Async<Option<<Self as Stream>::Item>>, <Self as Stream>::Error> {
        loop {
            match self.state {
                State::Clean => match try_ready!(self.upstream.poll()) {
                    Some(RawTarItem::Header(header)) => return self.poll_next_header(header),
                    None => return Ok(Async::Ready(None)),
                    Some(RawTarItem::Chunk(bytes)) => {
                        return Ok(Async::Ready(Some(TarItem::Chunk(bytes))))
                    }
                    Some(RawTarItem::EmptyHeader) => (),
                },
                _ => return self.poll_data(),
            }
        }
    }
}

impl<E: Debug + Send + Sync + 'static, U: Stream<Item = RawTarItem, Error = Error<E>>>
    EntryStream<U>
{
    fn new(upstream: U) -> Self {
        EntryStream {
            upstream,
            buffer: None,
            gnu_long_name: None,
            gnu_long_link: None,
            pax_extensions: None,
            state: State::Clean,
            processed: 0,
        }
    }

    fn poll_next_header(
        &mut self,
        entry: tar::Header,
    ) -> Result<Async<Option<<Self as Stream>::Item>>, <Self as Stream>::Error> {
        eprintln!("header={:?}, s={:?}", entry, self.state);
        match (&self.state, self.buffer.take()) {
            (State::InGnuLongName, Some(buf)) => {
                self.gnu_long_name = Some(buf.to_vec());
            }
            (State::InGnuLongLink, Some(buf)) => {
                self.gnu_long_link = Some(buf.to_vec());
            }
            (State::InPaxExtensions, Some(buf)) => {
                self.pax_extensions = Some(buf.to_vec());
            }
            (State::Clean, _) => {}
            _state => unreachable!(),
        };
        self.state = State::Clean;

        if entry.as_gnu().is_some() && entry.entry_type().is_gnu_longname() {
            if self.gnu_long_name.is_some() {
                return Err(Error::Format(
                    "two long name entries describing \
                     the same member",
                ));
            }
            // TODO: Check max size
            self.buffer = Some(BytesMut::with_capacity(
                entry.size().map_err(|e| Error::IoError(e))? as usize,
            ));
            self.state = State::InGnuLongName;
            return self.poll_data();
        }
        if entry.as_gnu().is_some() && entry.entry_type().is_gnu_longlink() {
            if self.gnu_long_link.is_some() {
                return Err(Error::Format(
                    "two long name entries describing \
                     the same member",
                ));
            }
            self.buffer = Some(BytesMut::with_capacity(
                entry.size().map_err(|e| Error::IoError(e))? as usize,
            ));
            self.state = State::InGnuLongLink;
            return self.poll_data();
        }
        if entry.as_ustar().is_some() && entry.entry_type().is_pax_local_extensions() {
            if self.pax_extensions.is_some() {
                return Err(Error::Format(
                    "two pax extensions entries describing \
                     the same member",
                ));
            }
            self.buffer = Some(BytesMut::with_capacity(
                entry.size().map_err(|e| Error::IoError(e))? as usize,
            ));
            self.state = State::InPaxExtensions;
            return self.poll_data();
        }

        let path_bytes = self
            .gnu_long_name
            .take()
            .unwrap_or(entry.path_bytes().into());
        let link_bytes = self
            .gnu_long_link
            .take()
            .or_else(|| entry.link_name_bytes().map(|b| b.into()));
        let pax_extensions = self.pax_extensions.take();
        Ok(Async::Ready(Some(TarItem::Entry(TarEntry {
            path_bytes,
            link_bytes,
            pax_extensions,
            size: entry.size().map_err(|e| Error::IoError(e))?,
        }))))
    }

    fn poll_data(
        &mut self,
    ) -> Result<Async<Option<<Self as Stream>::Item>>, <Self as Stream>::Error> {
        assert_ne!(self.state, State::Clean);
        loop {
            match try_ready!(self.upstream.poll()) {
                Some(RawTarItem::Chunk(bytes)) => self.buffer.as_mut().unwrap().put(bytes),
                Some(RawTarItem::Header(header)) => return self.poll_next_header(header),
                Some(RawTarItem::EmptyHeader) => return Err(Error::UnexpectedEof),
                None => return Err(Error::UnexpectedEof),
            }
        }
    }
}

pub fn decode_tar<TarStream: Stream<Item = Bytes>>(
    upstream: TarStream,
) -> impl Stream<Item = TarItem, Error = Error<TarStream::Error>>
where
    TarStream::Error: std::fmt::Debug + Sync + Send + 'static,
{
    EntryStream::new(raw::decode_tar(upstream))
}
