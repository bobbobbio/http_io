use crate::protocol::HttpStatus;
use std::error;
use std::fmt;
use std::io;
use std::num;
use std::str;

#[derive(Debug)]
pub enum Error {
    ParseError(String),
    ParseIntError(num::ParseIntError),
    Utf8Error(str::Utf8Error),
    IoError(io::Error),
    UnexpectedEof(String),
    UnexpectedStatus(HttpStatus),
    UrlError(String),
}

pub type Result<R> = std::result::Result<R, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::ParseError(_) => None,
            Error::IoError(e) => Some(e),
            Error::Utf8Error(e) => Some(e),
            Error::ParseIntError(e) => Some(e),
            Error::UnexpectedEof(_) => None,
            Error::UnexpectedStatus(_) => None,
            Error::UrlError(_) => None,
        }
    }
}

impl From<str::Utf8Error> for Error {
    fn from(e: str::Utf8Error) -> Self {
        Error::Utf8Error(e)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::IoError(e)
    }
}

impl From<num::ParseIntError> for Error {
    fn from(e: num::ParseIntError) -> Self {
        Error::ParseIntError(e)
    }
}

impl<W> From<io::IntoInnerError<W>> for Error {
    fn from(e: io::IntoInnerError<W>) -> Self {
        Error::IoError(io::Error::new(
            io::ErrorKind::Other,
            format!("{}", e.error()),
        ))
    }
}
