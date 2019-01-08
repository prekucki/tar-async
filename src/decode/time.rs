use failure::Fail;
use std::{convert, str, time};

#[derive(Debug, Fail)]
pub enum ParseError {
    #[fail(display = "invalid timestamp char {}", 0)]
    InvalidChar(u8),
    #[fail(display = "overflow")]
    Overflow,
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub struct FileTime(u64, u32);

impl FileTime {
    #[inline]
    pub fn secs(&self) -> u64 {
        self.0
    }

    #[inline]
    pub fn subsec_nanos(&self) -> u32 {
        self.1
    }

    #[inline]
    pub fn from_secs(secs: u64) -> Self {
        FileTime(secs, 0)
    }

    #[inline]
    pub fn into_system_time(self) -> time::SystemTime {
        time::UNIX_EPOCH + time::Duration::new(self.0, self.1)
    }
}

impl From<u64> for FileTime {
    #[inline]
    fn from(secs: u64) -> Self {
        Self::from_secs(secs)
    }
}

impl Into<time::SystemTime> for FileTime {
    fn into(self) -> time::SystemTime {
        self.into_system_time()
    }
}

impl str::FromStr for FileTime {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_from(s.as_ref())
    }
}

fn parse_from(value: &[u8]) -> Result<FileTime, ParseError> {
    let mut u = 0u64;

    let mut i = 0;
    // skip spaces
    while i < value.len() && (value[i] as char).is_whitespace() {
        i += 1;
    }

    while i < value.len() {
        let b = value[i];
        if b >= b'0' && b <= b'9' {
            u = u
                .checked_mul(10)
                .ok_or(ParseError::Overflow)?
                .checked_add((b - b'0') as u64)
                .ok_or(ParseError::Overflow)?;
            i += 1;
        } else {
            break;
        }
    }

    if i < value.len() && value[i] == b'.' {
        let mut d = 0u64;
        let mut ds = 1;
        i += 1;
        while i < value.len() {
            let b = value[i];
            if b >= b'0' && b <= b'9' {
                d = d
                    .checked_mul(10)
                    .ok_or(ParseError::Overflow)?
                    .checked_add((b - b'0') as u64)
                    .ok_or(ParseError::Overflow)?;
                ds += 1;
                i += 1;
            } else {
                break;
            }
        }
        if i != value.len() {
            return Err(ParseError::InvalidChar(value[i]));
        }
        while ds < 9 {
            d *= 10;
            ds += 1;
        }
        Ok(FileTime(u, d as u32))
    } else {
        Ok(FileTime(u, 0))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse() {
        assert_eq!(parse_from(b"123").unwrap(), FileTime(123, 0));
        assert_eq!(
            parse_from(b"   556677.2").unwrap(),
            FileTime(556677, 20000000)
        );

        let s: time::SystemTime = parse_from(b"   1546952073.491116718").unwrap().into();
        eprintln!("s={:?}", time::SystemTime::now());
        eprintln!("s={:?}", s);
    }

}
