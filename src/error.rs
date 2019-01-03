use failure::Fail;
use std::io;

#[derive(Debug, Fail)]
pub enum Error<E: std::fmt::Debug + Sync + Send + 'static> {
    #[fail(display = "{:?}", 0)]
    UpstreamError(E),
    #[fail(display = "{}", 0)]
    IoError(io::Error),
    #[fail(display = "UnexpectedEof")]
    UnexpectedEof,
    #[fail(display = "format error: {}", 0)]
    Format(&'static str),
}

impl<E: std::fmt::Debug + Sync + Send + 'static> From<E> for Error<E> {
    fn from(e: E) -> Self {
        Error::UpstreamError(e)
    }
}

/*
impl<E: std::fmt::Debug + Sync + Send + 'static> From<io::Error> for Error<E> {
    fn from(e : io::Error) -> Self {
        Error::IoError(e)
    }
}
*/
