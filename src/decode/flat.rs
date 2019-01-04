use super::pax::{PaxAttributes, PaxDecoder};
use super::raw::{self, RawTarItem};
use super::Error;
use bytes::{BufMut, Bytes, BytesMut};
use futures::{prelude::*, try_ready};
use std::fmt::{self, Debug, Formatter};
use std::mem;
use std::path::Path;
use std::{io, str};

fn bytes2path(bytes: &[u8]) -> io::Result<&Path> {
    let s = str::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    Ok(Path::new(s))
}

fn gnu_str_buffer2vec(buf: Bytes) -> Vec<u8> {
    match buf.last() {
        Some(0) => &buf.as_ref()[..buf.len() - 1],
        _ => buf.as_ref(),
    }
    .into()
}

pub struct TarEntry {
    entry_type: tar::EntryType,
    path_bytes: Vec<u8>,
    link_bytes: Option<Vec<u8>>,
    atime: Option<f64>,
    ctime: Option<f64>,
    mtime: f64,
    uid: u64,
    uname: Option<Vec<u8>>,
    gid: u64,
    gname: Option<Vec<u8>>,
    size: u64,
}

impl TarEntry {
    #[inline]
    pub fn entry_type(&self) -> tar::EntryType {
        self.entry_type
    }

    #[inline]
    pub fn path(&self) -> io::Result<&Path> {
        bytes2path(self.path_bytes.as_slice())
    }

    #[inline]
    pub fn link(&self) -> io::Result<Option<&Path>> {
        Ok(match self.link_bytes.as_ref() {
            Some(bytes) => Some(bytes2path(bytes)?),
            None => None,
        })
    }

    #[inline]
    pub fn size(&self) -> u64 {
        self.size
    }

    #[inline]
    pub fn uid(&self) -> u64 {
        self.uid
    }

    #[inline]
    pub fn gid(&self) -> u64 {
        self.gid
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
            "Entry {} entry_type={:?} path={:?}, link={:?}, size={:?}, uid={}, gid={} {}",
            '{',
            self.entry_type(),
            self.path(),
            self.link(),
            self.size(),
            self.uid(),
            self.gid(),
            '}'
        )
    }
}

struct EntryStream<U> {
    upstream: U,
    buffer: Option<BytesMut>,
    attributes: PaxAttributes,
    state: State,
    processed: u32,
}

#[derive(Debug)]
enum State {
    Clean,
    InGnuLongName,
    InGnuLongLink,
    InPaxExtensions(Box<PaxDecoder>),
}

impl State {
    #[inline]
    fn take(&mut self) -> State {
        mem::replace(self, State::Clean)
    }
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
            attributes: PaxAttributes::default(),
            state: State::Clean,
            processed: 0,
        }
    }

    fn poll_next_header(
        &mut self,
        entry: tar::Header,
    ) -> Result<Async<Option<<Self as Stream>::Item>>, <Self as Stream>::Error> {
        match (self.state.take(), self.buffer.take()) {
            (State::InGnuLongName, Some(buf)) => {
                self.attributes.path = Some(gnu_str_buffer2vec(buf.freeze()));
            }
            (State::InGnuLongLink, Some(buf)) => {
                self.attributes.link_path = Some(gnu_str_buffer2vec(buf.freeze()));
            }
            (State::InPaxExtensions(decoder), None) => {
                self.attributes = decoder.into_attr();
            }
            (State::Clean, _) => {}
            _state => unreachable!(),
        };

        if entry.as_gnu().is_some() && entry.entry_type().is_gnu_longname() {
            // TODO: Check max size
            self.buffer = Some(BytesMut::with_capacity(
                entry.size().map_err(|e| Error::IoError(e))? as usize,
            ));
            self.state = State::InGnuLongName;
            return self.poll_data();
        }
        if entry.as_gnu().is_some() && entry.entry_type().is_gnu_longlink() {
            self.buffer = Some(BytesMut::with_capacity(
                entry.size().map_err(|e| Error::IoError(e))? as usize,
            ));
            self.state = State::InGnuLongLink;
            return self.poll_data();
        }
        if entry.as_ustar().is_some() && entry.entry_type().is_pax_local_extensions() {
            self.buffer = None;
            self.state = State::InPaxExtensions(Box::new(PaxDecoder::new()));
            return self.poll_data();
        }

        if let Some(header) = entry.as_gnu() {
            self.attributes.atime = header.atime().ok().map(|v| v as f64);
            self.attributes.ctime = header.ctime().ok().map(|v| v as f64);
        }

        let path_bytes = self
            .attributes
            .path
            .take()
            .unwrap_or(entry.path_bytes().into());
        let link_bytes = self
            .attributes
            .link_path
            .take()
            .or_else(|| entry.link_name_bytes().map(|b| b.into()));

        let size = self
            .attributes
            .size
            .take()
            .unwrap_or(entry.size().map_err(|e| Error::IoError(e))?);

        let uid = match self.attributes.uid.take() {
            Some(uid) => uid,
            None => entry.uid().map_err(|e| Error::IoError(e))?,
        };
        let gid = match self.attributes.gid.take() {
            Some(gid) => gid,
            None => entry.gid().map_err(|e| Error::IoError(e))?,
        };
        let mtime = match self.attributes.mtime.take() {
            Some(mtime) => mtime,
            None => entry.mtime().map_err(|e| Error::IoError(e))? as f64,
        };

        let ctime = self.attributes.ctime.take();
        let atime = self.attributes.atime.take();

        let uname = self
            .attributes
            .uname
            .take()
            .or_else(|| entry.username_bytes().map(|b| b.into()));
        let gname = self
            .attributes
            .gname
            .take()
            .or_else(|| entry.groupname_bytes().map(|b| b.into()));

        Ok(Async::Ready(Some(TarItem::Entry(TarEntry {
            entry_type: entry.entry_type(),
            path_bytes,
            link_bytes,
            size,
            gid,
            uid,
            mtime,
            ctime,
            atime,
            uname,
            gname,
        }))))
    }

    fn poll_data(
        &mut self,
    ) -> Result<Async<Option<<Self as Stream>::Item>>, <Self as Stream>::Error> {
        loop {
            match try_ready!(self.upstream.poll()) {
                Some(RawTarItem::Chunk(bytes)) => match self.state {
                    State::InGnuLongLink | State::InGnuLongName => {
                        self.buffer.as_mut().unwrap().put(bytes)
                    }
                    State::InPaxExtensions(ref mut decoder) => decoder
                        .decode(bytes)
                        .map_err(|_| Error::Format("pax format"))?,
                    _ => unreachable!(),
                },
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
