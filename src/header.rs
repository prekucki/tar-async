
use tar::PaxExtensions;
use std::path::{Path, PathBuf};
use futures::prelude::*;
use futures::try_ready;
use bytes::{Bytes, BytesMut, Buf, BufMut};

pub enum Error<E> {
    UpstreamError(E),
    UnexpectedEof
}

impl<E> From<E> for Error<E> {
    fn from(e: E) -> Self {
        Error::UpstreamError(e)
    }
}

pub struct Config {
    unpack_xattrs : bool,
    preserve_permissions : bool,
    preserve_mtime : bool,
    ignore_zeros : bool
}

pub struct EntryStream<Upstream> {
    upstream : Upstream,
    buffer : BytesMut,
    tail : Option<Bytes>,
    config : Config
}

impl<Upstream : Stream<Item=Bytes>> EntryStream<Upstream> {

    fn fetch_header(&mut self) -> Result<Async<Option<()>>, Error<Upstream::Error>> {
        if Some(tail) = self.tail.take() {
            let rem = 512 - self.buffer.len();
            if chunk.len() < rem {
                self.buffer.put(chunk);
                return Ok(Async::NotReady)
            }

            self.buffer.put(chunk.take(rem));
            self.tail = Some(chunk);
            return Ok(Async::Ready(Some(())))
        }
        if let Some(mut chunk) = try_ready!(self.upstream.poll()) {
            let rem = 512 - self.buffer.len();
            if chunk.len() < rem {
                self.buffer.put(chunk);
                return Ok(Async::NotReady)
            }

            self.buffer.put(chunk.take(rem));
            self.tail = Some(chunk);
            Ok(Async::Ready(Some(())))
        }
        else {
            Err(Error::UnexpectedEof)
        }
    }

}

impl<Upstream : Stream<Item=Bytes>> Stream for EntryStream<Upstream> {
    type Item = AsyncEntry;
    type Error = Error<Upstream::Error>;

    fn poll(&mut self) -> Result<Async<Option<<Self as Stream>::Item>>, <Self as Stream>::Error> {
        if self.buffer.len() < 512 {

        }
    }
}

pub struct AsyncEntry {
    file_name : PathBuf,
    link_name : Option<PathBuf>,
    extensions : Option<Vec<u8>>,
}