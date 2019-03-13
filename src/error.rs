use crate::protocol::HttpStatus;
use std::convert;
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
        }
    }
}

impl convert::From<str::Utf8Error> for Error {
    fn from(e: str::Utf8Error) -> Self {
        Error::Utf8Error(e)
    }
}

impl convert::From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::IoError(e)
    }
}

impl convert::From<num::ParseIntError> for Error {
    fn from(e: num::ParseIntError) -> Self {
        Error::ParseIntError(e)
    }
}
