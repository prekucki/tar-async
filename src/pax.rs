use bytes::{BufMut, Bytes, BytesMut};
use failure::Fail;
use std::str::FromStr;
use std::{io, mem};

#[derive(Debug, Fail)]
pub enum ParseError {
    #[fail(display = "invalid size char '{}'", 0)]
    InvalidSizeChar(char),
    #[fail(display = "overflow")]
    Overflow,
    #[fail(display = "expected eol")]
    ExpectedEol,
    #[fail(display = "expected '='")]
    ExpectedEq,
    #[fail(display = "utf8 expected")]
    ExpectedUtf8,
}

#[derive(Default, Debug)]
pub struct PaxAttributes {
    pub path: Option<Vec<u8>>,
    pub link_path: Option<Vec<u8>>,
    pub atime: Option<f64>,
    pub ctime: Option<f64>,
    pub mtime: Option<f64>,
    pub uid: Option<u64>,
    pub uname: Option<Vec<u8>>,
    pub gid: Option<u64>,
    pub gname: Option<Vec<u8>>,
    pub size: Option<u64>,
}

impl PaxAttributes {
    fn decode_record(&mut self, record: &[u8]) -> Result<(), ParseError> {
        eprintln!("record='{}'", std::str::from_utf8(record).unwrap());
        let (key, val) = cut_sep(record, b'=').ok_or_else(|| ParseError::ExpectedEq)?;
        let val = &val[1..];
        Ok(match key {
            b"path" => self.path = Some(val.into()),
            b"linkpath" => self.link_path = Some(val.into()),
            b"mtime" => self.mtime = Some(parse_str(val)?),
            b"ctime" => self.ctime = Some(parse_str(val)?),
            b"atime" => self.atime = Some(parse_str(val)?),
            b"size" => self.size = Some(parse_str(val)?),
            b"uid" => self.uid = Some(parse_str(val)?),
            b"gid" => self.gid = Some(parse_str(val)?),
            b"uname" => self.uname = Some(val.into()),
            b"gname" => self.gname = Some(val.into()),
            _ => return Ok(()),
        })
    }
}

pub struct PaxDecoder {
    attributes: PaxAttributes,
    buffer: BytesMut,
    adv: usize,
}

fn cut_sep(bytes: &[u8], sep: u8) -> Option<(&[u8], &[u8])> {
    bytes
        .iter()
        .position(|it| *it == sep)
        .map(|position| bytes.split_at(position))
}

fn parse_str<T: FromStr>(bytes: &[u8]) -> Result<T, ParseError> {
    use std::str;

    let s = str::from_utf8(bytes).map_err(|_| ParseError::ExpectedUtf8)?;

    Ok(s.parse().map_err(|e| ParseError::ExpectedUtf8)?)
}

fn parse_size(bytes: &[u8]) -> Result<u64, ParseError> {
    let mut val = 0u64;
    for b in bytes {
        val = if *b >= b'0' && *b <= b'9' {
            val.checked_mul(10).ok_or_else(|| ParseError::Overflow)? + (*b - b'0') as u64
        } else {
            return Err(ParseError::InvalidSizeChar(*b as char));
        }
    }
    Ok(val)
}

fn cut_record(bytes: &[u8]) -> Result<Option<(usize, &[u8], &[u8])>, ParseError> {
    let (size_bytes, tail_bytes) = match cut_sep(bytes, b' ') {
        Some(v) => v,
        None => return Ok(None),
    };
    let size = parse_size(size_bytes)?;
    if size < size_bytes.len() as u64 {
        return Ok(None);
    }

    let size = size - size_bytes.len() as u64;
    if tail_bytes.len() as u64 >= size {
        let size = size as usize;
        if tail_bytes[size - 1] != b'\n' {
            return Err(ParseError::ExpectedEol);
        }
        let record = &tail_bytes[1..size - 1];
        let tail = &tail_bytes[size..];

        Ok(Some((size, record, tail)))
    } else {
        Ok(None)
    }
}

impl PaxDecoder {
    pub fn new() -> Self {
        PaxDecoder {
            attributes: PaxAttributes::default(),
            buffer: BytesMut::with_capacity(1024),
            adv: 0,
        }
    }

    pub fn decode(&mut self, bytes: Bytes) -> Result<(), ParseError> {
        if self.adv > 0 {
            self.buffer.advance(mem::replace(&mut self.adv, 0))
        }
        self.buffer.reserve(bytes.len());
        self.buffer.put(bytes);
        let mut bb = self.buffer.as_ref();
        loop {
            eprintln!("bb={:?}", bb);
            if let Some((n, record, b)) = cut_record(bb)? {
                eprintln!("data");
                self.adv += n;
                self.attributes.decode_record(record)?;
                bb = b;
            } else {
                eprintln!("no data");
                break;
            }
        }
        Ok(())
    }

    pub fn into_attr(self) -> PaxAttributes {
        self.attributes
    }
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn test_parse() {
        let mut bytes = b"20 path=ala/ma/kota\n30 mtime=1546272612.201798006\n30 atime=1546272612.201798006\n30 ctime=1546272612.201798006\n".as_ref();
        /*while bytes.len() > 0 {
            if let Some((n,a,b)) = cut_record(bytes).unwrap() {
                eprintln!("record={} '{}'", n, std::str::from_utf8(a).unwrap());
                bytes = b;
            }
            else {
                eprintln!("tail='{}'", std::str::from_utf8(bytes).unwrap());
                break;
            }
        }*/

        let mut decoder = PaxDecoder::new();
        decoder.decode(Bytes::from_static(bytes)).unwrap();
        eprintln!("{:?}", decoder.into_attr())
    }

    #[test]
    fn test_buf() {
        let mut bm = BytesMut::new();
        let kot = Bytes::from(b"ala ma kota\n".as_ref());

        eprintln!("bm.cap={} {}", bm.capacity(), bm.len());
        bm.reserve(kot.len() * 3);
        bm.put(&kot);
        bm.reserve(0);
        bm.put(&kot);
        bm.put(&kot);

        eprintln!("bm.cap={} {}", bm.capacity(), bm.len());
        eprintln!("bm='{}'", std::str::from_utf8(bm.as_ref()).unwrap());
        bm.advance(kot.len() * 2);
        eprintln!("bm.cap={} {}", bm.capacity(), bm.len());
        eprintln!("bm='{}'", std::str::from_utf8(bm.as_ref()).unwrap());
        bm.reserve(kot.len() * 2);
        eprintln!("bm.cap={} {}", bm.capacity(), bm.len());
        eprintln!("bm='{}'", std::str::from_utf8(bm.as_ref()).unwrap());

        bm.put(&kot);
        bm.put(&kot);
        eprintln!("bm.cap={} {}", bm.capacity(), bm.len());
        eprintln!("bm='{}'", std::str::from_utf8(bm.as_ref()).unwrap());
    }

}
