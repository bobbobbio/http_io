use crate::protocol::{HttpMethod, HttpStatus};
#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::fmt;
use core::num;
use core::str;

#[derive(Debug)]
pub enum Error {
    ParseError(String),
    ParseIntError(num::ParseIntError),
    Utf8Error(str::Utf8Error),
    UnexpectedScheme(String),
    UnexpectedEof(String),
    UnexpectedStatus(HttpStatus),
    UnexpectedMethod(HttpMethod),
    UrlError(String),
    Other(String),

    #[cfg(feature = "std")]
    /// *This variant is available if http_io is built with the `"std"` feature.*
    IoError(std::io::Error),

    #[cfg(feature = "openssl")]
    SslError(String),
}

pub type Result<R> = core::result::Result<R, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<str::Utf8Error> for Error {
    fn from(e: str::Utf8Error) -> Self {
        Error::Utf8Error(e)
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IoError(e)
    }
}

impl From<num::ParseIntError> for Error {
    fn from(e: num::ParseIntError) -> Self {
        Error::ParseIntError(e)
    }
}

#[cfg(feature = "std")]
impl<W> From<std::io::IntoInnerError<W>> for Error {
    fn from(e: std::io::IntoInnerError<W>) -> Self {
        Error::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("{}", e.error()),
        ))
    }
}

#[cfg(feature = "std")]
impl From<Error> for std::io::Error {
    fn from(e: Error) -> Self {
        std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
    }
}

#[cfg(feature = "openssl")]
impl From<openssl::error::ErrorStack> for Error {
    fn from(e: openssl::error::ErrorStack) -> Self {
        Error::SslError(e.to_string())
    }
}

#[cfg(feature = "openssl")]
impl<S: fmt::Debug> From<openssl::ssl::HandshakeError<S>> for Error {
    fn from(e: openssl::ssl::HandshakeError<S>) -> Self {
        Error::SslError(e.to_string())
    }
}
