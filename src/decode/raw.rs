use super::Error;
use bytes::{BufMut, Bytes, BytesMut};
use futures::{prelude::*, try_ready};
use tar::Header;

#[derive(Debug)]
pub enum RawTarItem {
    Header(Header),
    EmptyHeader,
    Chunk(Bytes),
}

const HEADER_SIZE: usize = 512;

struct RawTarStream<Upstream> {
    upstream: Upstream,
    buffer: BytesMut,
    tail: Option<Bytes>,
    in_entry_raw: u64,
    in_entry: u64,
}

impl<Upstream: Stream<Item = Bytes>> RawTarStream<Upstream>
where
    Upstream::Error: std::fmt::Debug + Sync + Send + 'static,
{
    fn new(upstream: Upstream) -> Self {
        RawTarStream {
            upstream,
            buffer: BytesMut::with_capacity(HEADER_SIZE),
            tail: None,
            in_entry_raw: 0,
            in_entry: 0,
        }
    }

    fn fetch_entry_bytes(&mut self) -> Result<Async<Option<Bytes>>, Error<Upstream::Error>> {
        if let Some(mut tail) = self.tail.take() {
            if tail.len() as u64 <= self.in_entry {
                self.in_entry -= tail.len() as u64;
                Ok(Async::Ready(Some(tail)))
            } else {
                assert!(self.in_entry < std::usize::MAX as u64);
                let mut chunk = tail.split_to(self.in_entry as usize);
                self.in_entry = 0;
                self.tail = Some(tail);
                Ok(Async::Ready(Some(chunk)))
            }
        } else {
            if let Some(mut bytes) = try_ready!(self.upstream.poll()) {
                if bytes.len() as u64 <= self.in_entry {
                    self.in_entry -= bytes.len() as u64;
                    Ok(Async::Ready(Some(bytes)))
                } else {
                    let head = bytes.split_to(self.in_entry as usize);
                    self.in_entry = 0;
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
                        header
                            .as_mut_bytes()
                            .copy_from_slice(tail.split_to(HEADER_SIZE).as_ref());
                        self.tail = Some(tail);
                        return Ok(Async::Ready(Some(header)));
                    } else {
                        let output_buf = self.buffer.as_ref();
                        let header_bytes = header.as_mut_bytes();
                        header_bytes[..output_buf.len()].copy_from_slice(output_buf);

                        header_bytes[output_buf.len()..].copy_from_slice(
                            tail.split_to(HEADER_SIZE - output_buf.len()).as_ref(),
                        );

                        self.buffer.clear();
                        self.tail = Some(tail);
                        return Ok(Async::Ready(Some(header)));
                    }
                } else {
                    self.buffer.put(tail);
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

impl<Upstream: Stream<Item = Bytes>> Stream for RawTarStream<Upstream>
where
    Upstream::Error: std::fmt::Debug + Sync + Send + 'static,
{
    type Item = RawTarItem;
    type Error = Error<Upstream::Error>;

    fn poll(&mut self) -> Result<Async<Option<<Self as Stream>::Item>>, <Self as Stream>::Error> {
        if self.in_entry > 0 {
            loop {
                if let Some(mut bytes) = try_ready!(self.fetch_entry_bytes()) {
                    if self.in_entry_raw >= bytes.len() as u64 {
                        self.in_entry_raw -= bytes.len() as u64;
                        return Ok(Async::Ready(Some(RawTarItem::Chunk(bytes))));
                    } else {
                        if self.in_entry_raw > 0 {
                            let chunk_size = self.in_entry_raw as usize;
                            self.in_entry_raw = 0;
                            return Ok(Async::Ready(Some(RawTarItem::Chunk(
                                bytes.split_to(chunk_size),
                            ))));
                        }
                        // read more
                    }
                } else {
                    // TODO: Eof
                    return Ok(Async::Ready(None));
                }
            }
        }

        if let Some(header) = try_ready!(self.fetch_header()) {
            if header.as_bytes().iter().all(|i| *i == 0) {
                Ok(Async::Ready(Some(RawTarItem::EmptyHeader)))
            } else {
                let size = match header.entry_size() {
                    Ok(size) => size,
                    Err(e) => return Err(Error::IoError(e)),
                };

                self.in_entry = (size + 511) & !(512 - 1);
                self.in_entry_raw = size;

                Ok(Async::Ready(Some(RawTarItem::Header(header))))
            }
        } else {
            Ok(Async::Ready(None))
        }
    }
}

pub fn decode_tar<TarStream: Stream<Item = Bytes>>(
    upstream: TarStream,
) -> impl Stream<Item = RawTarItem, Error = Error<TarStream::Error>>
where
    TarStream::Error: std::fmt::Debug + Sync + Send + 'static,
{
    RawTarStream::new(upstream)
}
