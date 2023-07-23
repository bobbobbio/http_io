//! Types representing various parts of the HTTP protocol.

// We do write! + '\r\n' and don't want to hide the line ending in a writeln!
#![allow(clippy::write_with_newline)]

use crate::error::{Error, Result};
use crate::io::{self, Read, Write};
#[cfg(not(feature = "std"))]
use alloc::{
    boxed::Box,
    collections::{btree_map::Iter as BTreeMapIter, BTreeMap},
    format,
    string::String,
    vec,
    vec::Vec,
};
use core::cmp;
use core::convert;
use core::fmt;
use core::iter;
use core::str;
#[cfg(feature = "std")]
use std::collections::{btree_map::Iter as BTreeMapIter, BTreeMap};

struct HttpBodyChunk<S: io::Read> {
    inner: io::Take<HttpReadTilCloseBody<S>>,
}

pub struct HttpChunkedBody<S: io::Read> {
    content_length: Option<u64>,
    stream: Option<HttpReadTilCloseBody<S>>,
    chunk: Option<HttpBodyChunk<S>>,
}

impl<S: io::Read> HttpChunkedBody<S> {
    fn new(content_length: Option<u64>, stream: HttpReadTilCloseBody<S>) -> Self {
        HttpChunkedBody {
            content_length,
            stream: Some(stream),
            chunk: None,
        }
    }
}

impl<S: io::Read> HttpBodyChunk<S> {
    fn new(mut stream: HttpReadTilCloseBody<S>) -> Result<Option<Self>> {
        let mut ts = CrLfStream::new(&mut stream);
        let size_str = ts.expect_next()?;
        drop(ts);
        let size = u64::from_str_radix(&size_str, 16)?;
        Ok(if size == 0 {
            None
        } else {
            Some(HttpBodyChunk {
                inner: stream.take(size),
            })
        })
    }

    fn into_inner(self) -> HttpReadTilCloseBody<S> {
        self.inner.into_inner()
    }
}

impl<S: io::Read> io::Read for HttpBodyChunk<S> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buffer)
    }
}

impl<S: io::Read> io::Read for HttpChunkedBody<S> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if let Some(mut chunk) = self.chunk.take() {
            let read = chunk.read(buffer)?;
            if read == 0 {
                let mut stream = chunk.into_inner();
                let mut b = [0; 2];
                stream.read_exact(&mut b)?;
                self.stream = Some(stream);
                self.read(buffer)
            } else {
                self.chunk.replace(chunk);
                Ok(read)
            }
        } else if let Some(stream) = self.stream.take() {
            let new_chunk = HttpBodyChunk::new(stream)?;
            match new_chunk {
                Some(chunk) => {
                    self.chunk = Some(chunk);
                    self.read(buffer)
                }
                None => Ok(0),
            }
        } else {
            Ok(0)
        }
    }
}

#[cfg(test)]
mod chunked_encoding_tests {
    use super::HttpChunkedBody;
    use crate::error::Result;
    use std::io;
    use std::io::Read;

    fn chunk_test(i: &'static str) -> Result<String> {
        let input = io::BufReader::new(io::Cursor::new(i));
        let mut body = HttpChunkedBody::new(None, input);

        let mut output = String::new();
        body.read_to_string(&mut output)?;
        Ok(output)
    }

    #[test]
    fn simple_chunk() {
        assert_eq!(
            &chunk_test("a\r\n0123456789\r\n0\r\n").unwrap(),
            "0123456789"
        );
    }

    #[test]
    fn chunk_missing_last_chunk() {
        assert!(chunk_test("a\r\n0123456789\r\n").is_err());
    }

    #[test]
    fn chunk_short_read() {
        assert!(chunk_test("a\r\n012345678").is_err());
    }
}

type HttpReadTilCloseBody<S> = io::BufReader<S>;
type HttpLimitedBody<S> = io::Take<HttpReadTilCloseBody<S>>;

pub enum HttpBody<S: io::Read> {
    Chunked(HttpChunkedBody<S>),
    Limited(HttpLimitedBody<S>),
    ReadTilClose(HttpReadTilCloseBody<S>),
}

impl<S: io::Read> io::Read for HttpBody<S> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            HttpBody::Chunked(i) => i.read(buffer),
            HttpBody::Limited(i) => i.read(buffer),
            HttpBody::ReadTilClose(i) => i.read(buffer),
        }
    }
}

impl<S: io::Read> HttpBody<S> {
    pub fn new(
        encoding: Option<&str>,
        content_length: Option<u64>,
        body: io::BufReader<S>,
    ) -> Self {
        if encoding == Some("chunked") {
            HttpBody::Chunked(HttpChunkedBody::new(content_length, body))
        } else if let Some(length) = content_length {
            HttpBody::Limited(body.take(length))
        } else {
            HttpBody::ReadTilClose(body)
        }
    }

    pub fn require_length(&self) -> Result<()> {
        let has_length = match self {
            HttpBody::Chunked(_) => true,
            HttpBody::Limited(_) => true,
            HttpBody::ReadTilClose(_) => false,
        };

        if !has_length {
            Err(Error::LengthRequired)
        } else {
            Ok(())
        }
    }

    pub fn content_length(&self) -> Option<u64> {
        match self {
            HttpBody::Chunked(c) => c.content_length.clone(),
            HttpBody::Limited(c) => Some(c.limit()),
            HttpBody::ReadTilClose(_) => None,
        }
    }
}

#[test]
fn chunked_body_no_content_length() {
    let body = HttpBody::new(Some("chunked"), None, io::BufReader::new(io::empty()));
    assert_eq!(body.content_length(), None);
}

#[test]
fn chunked_body_content_length() {
    let body = HttpBody::new(Some("chunked"), Some(12), io::BufReader::new(io::empty()));
    assert_eq!(body.content_length(), Some(12));
}

#[test]
fn read_till_close_body_has_no_content_length() {
    let body = HttpBody::new(None, None, io::BufReader::new(io::empty()));
    assert_eq!(body.content_length(), None);
}

#[test]
fn limited_body_content_length() {
    let body = HttpBody::new(None, Some(12), io::BufReader::new(io::empty()));
    assert_eq!(body.content_length(), Some(12));
}

pub struct CrLfStream<W> {
    stream: io::Bytes<W>,
}

impl<W: io::Read> CrLfStream<W> {
    pub fn new(stream: W) -> Self {
        CrLfStream {
            stream: stream.bytes(),
        }
    }
}

impl<W: io::Read> Iterator for CrLfStream<W> {
    type Item = Result<String>;
    fn next(&mut self) -> Option<Result<String>> {
        match self.inner_next() {
            Err(e) => Some(Err(e)),
            Ok(v) => v.map(Ok),
        }
    }
}

impl<W: io::Read> CrLfStream<W> {
    fn inner_next(&mut self) -> Result<Option<String>> {
        let mut line = Vec::new();
        while let Some(byte) = self.stream.next() {
            let byte = byte?;
            line.push(byte);
            if line.len() >= 2
                && line[line.len() - 2] as char == '\r'
                && line[line.len() - 1] as char == '\n'
            {
                let before = &line[..(line.len() - 2)];
                if before.is_empty() {
                    return Ok(None);
                } else {
                    return Ok(Some(str::from_utf8(before)?.into()));
                }
            }
        }
        Err(Error::UnexpectedEof("Expected \\r\\n".into()))
    }

    pub fn expect_next(&mut self) -> Result<String> {
        self.inner_next()?
            .ok_or_else(|| Error::UnexpectedEof("Expected line".into()))
    }
}

#[cfg(test)]
mod cr_lf_tests {
    use super::CrLfStream;

    #[test]
    fn success() {
        let input = "line1\r\nline2\r\n\r\n";
        let mut s = CrLfStream::new(input.as_bytes());
        assert_eq!(&s.next().unwrap().unwrap(), "line1");
        assert_eq!(&s.next().unwrap().unwrap(), "line2");
        assert!(s.next().is_none());
    }

    #[test]
    fn expect_next() {
        let input = "line1\r\nline2\r\n\r\n";
        let mut s = CrLfStream::new(input.as_bytes());
        assert_eq!(&s.expect_next().unwrap(), "line1");
        assert_eq!(&s.expect_next().unwrap(), "line2");
        assert!(s.expect_next().is_err());
    }

    #[test]
    fn fails_with_missing_empty_line() {
        let input = "line1\r\nline2\r\n";
        let mut s = CrLfStream::new(input.as_bytes());
        assert_eq!(&s.next().unwrap().unwrap(), "line1");
        assert_eq!(&s.next().unwrap().unwrap(), "line2");
        assert!(s.next().unwrap().is_err());
    }

    #[test]
    fn fails_finding_separator() {
        let input = "line1";
        let mut s = CrLfStream::new(input.as_bytes());
        assert!(s.next().unwrap().is_err());
    }
}

pub struct Parser<'a> {
    s: &'a str,
    position: usize,
}

impl<'a> Parser<'a> {
    pub fn new(s: &'a str) -> Self {
        Parser { s, position: 0 }
    }

    pub fn expect(&mut self, expected: &str) -> Result<()> {
        if self.position >= self.s.len() {
            return Err(Error::UnexpectedEof(format!("Expected {}", expected)));
        }

        let start = self.position;
        let end = cmp::min(self.s.len(), self.position + expected.len());
        let actual = &self.s[start..end];
        if actual != expected {
            return Err(Error::ParseError(format!(
                "Expected '{}', got '{}'",
                expected, actual
            )));
        }
        self.position += expected.len();
        Ok(())
    }

    pub fn parse_char(&mut self) -> Result<char> {
        if self.position >= self.s.len() {
            return Err(Error::UnexpectedEof("Expected char".into()));
        }

        let c = self.s[self.position..=self.position]
            .chars()
            .next()
            .unwrap();
        self.position += 1;
        Ok(c)
    }

    pub fn parse_digit(&mut self) -> Result<u32> {
        if self.position >= self.s.len() {
            return Err(Error::UnexpectedEof("Expected digit".into()));
        }

        let digit = &self.s[self.position..=self.position];
        self.position += 1;
        Ok(digit.parse()?)
    }

    pub fn parse_until(&mut self, div: &str) -> Result<&'a str> {
        if self.position >= self.s.len() {
            return Err(Error::UnexpectedEof(format!("Expected '{}'", div)));
        }

        let remaining = &self.s[self.position..];
        let pos = remaining
            .find(div)
            .ok_or_else(|| Error::ParseError(format!("Expected '{}' in '{}'", div, remaining)))?;
        self.position += pos;
        Ok(&remaining[..pos])
    }

    pub fn parse_until_any(&mut self, divs: &[char]) -> Result<&'a str> {
        if self.position >= self.s.len() {
            return Err(Error::UnexpectedEof(format!("Expected '{:?}'", divs)));
        }

        let remaining = &self.s[self.position..];
        let pos = remaining.find(|c| divs.contains(&c)).ok_or_else(|| {
            Error::ParseError(format!("Expected '{:?}' in '{}'", divs, remaining))
        })?;
        self.position += pos;
        Ok(&remaining[..pos])
    }

    pub fn consume_whilespace(&mut self) {
        while self.position < self.s.len()
            && (self.s[self.position..].starts_with(' ')
                || self.s[self.position..].starts_with('\t'))
        {
            self.position += 1
        }
    }

    pub fn parse_token(&mut self) -> Result<&'a str> {
        if self.position >= self.s.len() {
            return Err(Error::UnexpectedEof("Expected token".into()));
        }

        let remaining = &self.s[self.position..];
        let token = remaining.split(|c| c == ' ' || c == '\t').next().unwrap();
        self.position += token.len();
        self.consume_whilespace();

        Ok(token)
    }

    pub fn parse_number(&mut self) -> Result<u32> {
        Ok(self.parse_token()?.parse()?)
    }

    pub fn parse_remaining(&mut self) -> Result<&str> {
        if self.position > self.s.len() {
            return Err(Error::UnexpectedEof("Expected token".into()));
        }
        let remaining = &self.s[self.position..];
        self.position = self.s.len() + 1;
        Ok(remaining)
    }
}

#[cfg(test)]
mod parser_tests {
    use super::Parser;

    #[test]
    fn parse_empty() {
        let mut parser = Parser::new("");
        assert!(parser.expect("a").is_err());
        assert!(parser.parse_digit().is_err());
        assert!(parser.parse_token().is_err());
    }

    #[test]
    fn expect_success() {
        let mut parser = Parser::new("abcdefg");
        parser.expect("abc").unwrap();
        parser.expect("def").unwrap();
        parser.expect("g").unwrap();
    }

    #[test]
    fn expect_failure() {
        let mut parser = Parser::new("abcdefg");
        parser.expect("abc").unwrap();
        assert!(parser.expect("deg").is_err());
        parser.expect("defg").unwrap();
    }

    #[test]
    fn expect_failure_with_eof() {
        let mut parser = Parser::new("abcdefg");
        parser.expect("abcdefg").unwrap();
        assert!(parser.expect("a").is_err());
    }

    #[test]
    fn parse_token() {
        let mut parser = Parser::new("abc def");
        assert_eq!(parser.parse_token().unwrap(), "abc");
        assert_eq!(parser.parse_token().unwrap(), "def");
        assert!(parser.parse_token().is_err());
    }

    #[test]
    fn parse_token_with_lots_of_space() {
        let mut parser = Parser::new("abc  \t    def");
        assert_eq!(parser.parse_token().unwrap(), "abc");
        assert_eq!(parser.parse_token().unwrap(), "def");
        assert!(parser.parse_token().is_err());
    }

    #[test]
    fn parse_token_no_space() {
        let mut parser = Parser::new("abcdef");
        assert_eq!(parser.parse_token().unwrap(), "abcdef");
        assert!(parser.parse_token().is_err());
    }

    #[test]
    fn parse_until() {
        let mut parser = Parser::new("abc_def");
        assert_eq!(parser.parse_until("_").unwrap(), "abc");
        parser.expect("_").unwrap();
        assert!(parser.parse_until("_").is_err());
    }

    #[test]
    fn parse_until_any() {
        let mut parser = Parser::new("abc_def");
        assert_eq!(parser.parse_until_any(&['_', '-']).unwrap(), "abc");
        parser.expect("_").unwrap();
        assert!(parser.parse_until_any(&['_', '-']).is_err());

        let mut parser = Parser::new("abc-def");
        assert_eq!(parser.parse_until_any(&['_', '-']).unwrap(), "abc");
        parser.expect("-").unwrap();
        assert!(parser.parse_until_any(&['_', '-']).is_err());
    }

    #[test]
    fn parse_until_empty() {
        let mut parser = Parser::new("_abc");
        assert_eq!(parser.parse_until("_").unwrap(), "");
    }

    #[test]
    fn parse_until_no_divider() {
        let mut parser = Parser::new("abcdef");
        assert!(parser.parse_until("_").is_err());
    }

    #[test]
    fn parse_number() {
        let mut parser = Parser::new("123 456");
        assert_eq!(parser.parse_number().unwrap(), 123);
        assert_eq!(parser.parse_number().unwrap(), 456);
        assert!(parser.parse_number().is_err());
    }

    #[test]
    fn parse_number_failure() {
        let mut parser = Parser::new("123 abc");
        assert_eq!(parser.parse_number().unwrap(), 123);
        assert!(parser.parse_number().is_err());
    }

    #[test]
    fn parse_remaining() {
        let mut parser = Parser::new("123 abc");
        assert_eq!(parser.parse_remaining().unwrap(), "123 abc");
        assert!(parser.parse_remaining().is_err());
    }

    #[test]
    fn parse_remaining_empty() {
        let mut parser = Parser::new("123 abc");
        parser.expect("123 abc").unwrap();
        assert_eq!(parser.parse_remaining().unwrap(), "");
        assert!(parser.parse_remaining().is_err());
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct HttpVersion {
    major: u32,
    minor: u32,
}

impl HttpVersion {
    fn new(major: u32, minor: u32) -> Self {
        HttpVersion { major, minor }
    }
}

impl str::FromStr for HttpVersion {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let mut parser = Parser::new(s);
        parser.expect("HTTP/")?;

        let major = parser.parse_digit()?;
        parser.expect(".")?;
        let minor = parser.parse_digit()?;
        Ok(HttpVersion::new(major, minor))
    }
}

impl fmt::Display for HttpVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "HTTP/{}.{}", self.major, self.minor)
    }
}

#[cfg(test)]
mod http_version_tests {
    use super::HttpVersion;
    use std::string::ToString;

    #[test]
    fn parse_success() {
        assert_eq!(
            "HTTP/1.1".parse::<HttpVersion>().unwrap(),
            HttpVersion::new(1, 1)
        );
        assert_eq!(
            "HTTP/1.2".parse::<HttpVersion>().unwrap(),
            HttpVersion::new(1, 2)
        );
    }

    #[test]
    fn parse_error() {
        assert!("HTTP/".parse::<HttpVersion>().is_err());
        assert!("HTTP/11.1".parse::<HttpVersion>().is_err());
        assert!("HTTP/1".parse::<HttpVersion>().is_err());
        assert!("HRRP/1.2".parse::<HttpVersion>().is_err());
    }

    #[test]
    fn display() {
        assert_eq!(&HttpVersion::new(1, 3).to_string(), "HTTP/1.3");
    }

    #[test]
    fn parse_display_round_trip() {
        assert_eq!(
            &"HTTP/1.4".parse::<HttpVersion>().unwrap().to_string(),
            "HTTP/1.4"
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpStatus {
    Accepted,
    BadGateway,
    BadRequest,
    Conflict,
    Continue,
    Created,
    ExpectationFailed,
    Forbidden,
    Found,
    GatewayTimeout,
    Gone,
    HttpVersionNotSupported,
    InternalServerError,
    LengthRequired,
    MethodNotAllowed,
    MovedPermanently,
    MultipleChoices,
    NoContent,
    NonAuthoritativeInformation,
    NotAcceptable,
    NotFound,
    NotImplemented,
    NotModified,
    OK,
    PartialContent,
    PaymentRequired,
    PreconditionFailed,
    ProxyAuthenticationRequired,
    RequestEntityTooLarge,
    RequestTimeout,
    RequestUriTooLong,
    RequestedRangeNotSatisfiable,
    ResetContent,
    SeeOther,
    ServiceUnavailable,
    SwitchingProtocols,
    TemporaryRedirect,
    Unauthorized,
    UnsupportedMediaType,
    UseProxy,
    Unknown(u32),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HttpStatusCategory {
    Informational,
    Success,
    Redirection,
    ClientError,
    ServerError,
    Unknown,
}

impl HttpStatusCategory {
    fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Informational,
            2 => Self::Success,
            3 => Self::Redirection,
            4 => Self::ClientError,
            5 => Self::ServerError,
            _ => Self::Unknown,
        }
    }
}

impl HttpStatus {
    pub fn to_category(&self) -> HttpStatusCategory {
        HttpStatusCategory::from_code(self.to_code() / 100)
    }

    pub fn to_code(&self) -> u32 {
        match self {
            Self::Continue => 100,
            Self::SwitchingProtocols => 101,
            Self::OK => 200,
            Self::Created => 201,
            Self::Accepted => 202,
            Self::NonAuthoritativeInformation => 203,
            Self::NoContent => 204,
            Self::ResetContent => 205,
            Self::PartialContent => 206,
            Self::MultipleChoices => 300,
            Self::MovedPermanently => 301,
            Self::Found => 302,
            Self::SeeOther => 303,
            Self::NotModified => 304,
            Self::UseProxy => 305,
            Self::TemporaryRedirect => 307,
            Self::BadRequest => 400,
            Self::Unauthorized => 401,
            Self::PaymentRequired => 402,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::NotAcceptable => 406,
            Self::ProxyAuthenticationRequired => 407,
            Self::RequestTimeout => 408,
            Self::Conflict => 409,
            Self::Gone => 410,
            Self::LengthRequired => 411,
            Self::PreconditionFailed => 412,
            Self::RequestEntityTooLarge => 413,
            Self::RequestUriTooLong => 414,
            Self::UnsupportedMediaType => 415,
            Self::RequestedRangeNotSatisfiable => 416,
            Self::ExpectationFailed => 417,
            Self::InternalServerError => 500,
            Self::NotImplemented => 501,
            Self::BadGateway => 502,
            Self::ServiceUnavailable => 503,
            Self::GatewayTimeout => 504,
            Self::HttpVersionNotSupported => 505,
            Self::Unknown(c) => *c,
        }
    }

    pub fn from_code(code: u32) -> Self {
        match code {
            100 => Self::Continue,
            101 => Self::SwitchingProtocols,
            200 => Self::OK,
            201 => Self::Created,
            202 => Self::Accepted,
            203 => Self::NonAuthoritativeInformation,
            204 => Self::NoContent,
            205 => Self::ResetContent,
            206 => Self::PartialContent,
            300 => Self::MultipleChoices,
            301 => Self::MovedPermanently,
            302 => Self::Found,
            303 => Self::SeeOther,
            304 => Self::NotModified,
            305 => Self::UseProxy,
            307 => Self::TemporaryRedirect,
            400 => Self::BadRequest,
            401 => Self::Unauthorized,
            402 => Self::PaymentRequired,
            403 => Self::Forbidden,
            404 => Self::NotFound,
            405 => Self::MethodNotAllowed,
            406 => Self::NotAcceptable,
            407 => Self::ProxyAuthenticationRequired,
            408 => Self::RequestTimeout,
            409 => Self::Conflict,
            410 => Self::Gone,
            411 => Self::LengthRequired,
            412 => Self::PreconditionFailed,
            413 => Self::RequestEntityTooLarge,
            414 => Self::RequestUriTooLong,
            415 => Self::UnsupportedMediaType,
            416 => Self::RequestedRangeNotSatisfiable,
            417 => Self::ExpectationFailed,
            500 => Self::InternalServerError,
            501 => Self::NotImplemented,
            502 => Self::BadGateway,
            503 => Self::ServiceUnavailable,
            504 => Self::GatewayTimeout,
            505 => Self::HttpVersionNotSupported,
            v => Self::Unknown(v),
        }
    }
}

#[test]
fn category_from_status_code() {
    assert_eq!(
        HttpStatus::Continue.to_category(),
        HttpStatusCategory::Informational
    );
    assert_eq!(
        HttpStatus::Accepted.to_category(),
        HttpStatusCategory::Success
    );
    assert_eq!(
        HttpStatus::MovedPermanently.to_category(),
        HttpStatusCategory::Redirection
    );
    assert_eq!(
        HttpStatus::Forbidden.to_category(),
        HttpStatusCategory::ClientError
    );
    assert_eq!(
        HttpStatus::InternalServerError.to_category(),
        HttpStatusCategory::ServerError
    );

    assert_eq!(
        HttpStatus::Unknown(200).to_category(),
        HttpStatusCategory::Success
    );
    assert_eq!(
        HttpStatus::Unknown(700).to_category(),
        HttpStatusCategory::Unknown
    );
}

#[test]
fn from_code_to_code() {
    for c in 0..600 {
        assert_eq!(HttpStatus::from_code(c).to_code(), c);
    }
}

impl str::FromStr for HttpStatus {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let mut parser = Parser::new(s);
        Ok(Self::from_code(parser.parse_number()?))
    }
}

impl fmt::Display for HttpStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HttpStatus::Accepted => write!(f, "202 Accepted"),
            HttpStatus::BadGateway => write!(f, "502 Bad Gateway"),
            HttpStatus::BadRequest => write!(f, "400 Bad Request"),
            HttpStatus::Conflict => write!(f, "409 Conflict"),
            HttpStatus::Continue => write!(f, "100 Continue"),
            HttpStatus::Created => write!(f, "201 Created"),
            HttpStatus::ExpectationFailed => write!(f, "417 Expectation Failed"),
            HttpStatus::Forbidden => write!(f, "403 Forbidden"),
            HttpStatus::Found => write!(f, "302 Found"),
            HttpStatus::GatewayTimeout => write!(f, "504 Gateway Timeout"),
            HttpStatus::Gone => write!(f, "410 Gone"),
            HttpStatus::HttpVersionNotSupported => write!(f, "505 HTTP Version Not Supported"),
            HttpStatus::InternalServerError => write!(f, "500 Internal Server Error"),
            HttpStatus::LengthRequired => write!(f, "411 Length Required"),
            HttpStatus::MethodNotAllowed => write!(f, "405 Method Not Allowed"),
            HttpStatus::MovedPermanently => write!(f, "301 Moved Permanently"),
            HttpStatus::MultipleChoices => write!(f, "300 Multiple Choices"),
            HttpStatus::NoContent => write!(f, "204 No Content"),
            HttpStatus::NonAuthoritativeInformation => {
                write!(f, "203 No Authoritative Information")
            }
            HttpStatus::NotAcceptable => write!(f, "406 Not Acceptable"),
            HttpStatus::NotFound => write!(f, "404 Not Found"),
            HttpStatus::NotImplemented => write!(f, "501 Not Implemented"),
            HttpStatus::NotModified => write!(f, "304 NotModified"),
            HttpStatus::OK => write!(f, "200 OK"),
            HttpStatus::PartialContent => write!(f, "206 Partial Content"),
            HttpStatus::PaymentRequired => write!(f, "402 Payment Required"),
            HttpStatus::PreconditionFailed => write!(f, "412 Precondition Failed"),
            HttpStatus::ProxyAuthenticationRequired => {
                write!(f, "407 Prozy Authentication Required")
            }
            HttpStatus::RequestEntityTooLarge => write!(f, "413 Request Entity Too Large"),
            HttpStatus::RequestTimeout => write!(f, "408 Request Timeout"),
            HttpStatus::RequestUriTooLong => write!(f, "414 Request URI Too Long"),
            HttpStatus::RequestedRangeNotSatisfiable => {
                write!(f, "416 Requested Range Not Satisfiable")
            }
            HttpStatus::ResetContent => write!(f, "205 Reset Content"),
            HttpStatus::SeeOther => write!(f, "303 See Other"),
            HttpStatus::ServiceUnavailable => write!(f, "503 Service Unavailable"),
            HttpStatus::SwitchingProtocols => write!(f, "101 Switching Protocols"),
            HttpStatus::TemporaryRedirect => write!(f, "307 Temporary Redirect"),
            HttpStatus::Unauthorized => write!(f, "401 Unauthorized"),
            HttpStatus::UnsupportedMediaType => write!(f, "415 Unsupported Media Type"),
            HttpStatus::UseProxy => write!(f, "305 Use Proxy"),
            HttpStatus::Unknown(v) => write!(f, "{}", v),
        }
    }
}

#[cfg(test)]
mod http_status_tests {
    use super::HttpStatus;
    use std::string::ToString;

    #[test]
    fn parse_success() {
        assert_eq!(
            "301 Moved Permanently".parse::<HttpStatus>().unwrap(),
            HttpStatus::MovedPermanently,
        );
        assert_eq!("100".parse::<HttpStatus>().unwrap(), HttpStatus::Continue);
        assert_eq!(
            "101".parse::<HttpStatus>().unwrap(),
            HttpStatus::SwitchingProtocols
        );
        assert_eq!("200".parse::<HttpStatus>().unwrap(), HttpStatus::OK);
        assert_eq!("201".parse::<HttpStatus>().unwrap(), HttpStatus::Created);
        assert_eq!("202".parse::<HttpStatus>().unwrap(), HttpStatus::Accepted);
        assert_eq!(
            "203".parse::<HttpStatus>().unwrap(),
            HttpStatus::NonAuthoritativeInformation
        );
        assert_eq!("204".parse::<HttpStatus>().unwrap(), HttpStatus::NoContent);
        assert_eq!(
            "205".parse::<HttpStatus>().unwrap(),
            HttpStatus::ResetContent
        );
        assert_eq!(
            "206".parse::<HttpStatus>().unwrap(),
            HttpStatus::PartialContent
        );
        assert_eq!(
            "300".parse::<HttpStatus>().unwrap(),
            HttpStatus::MultipleChoices
        );
        assert_eq!(
            "301".parse::<HttpStatus>().unwrap(),
            HttpStatus::MovedPermanently
        );
        assert_eq!("302".parse::<HttpStatus>().unwrap(), HttpStatus::Found);
        assert_eq!("303".parse::<HttpStatus>().unwrap(), HttpStatus::SeeOther);
        assert_eq!(
            "304".parse::<HttpStatus>().unwrap(),
            HttpStatus::NotModified
        );
        assert_eq!("305".parse::<HttpStatus>().unwrap(), HttpStatus::UseProxy);
        assert_eq!(
            "307".parse::<HttpStatus>().unwrap(),
            HttpStatus::TemporaryRedirect
        );
        assert_eq!("400".parse::<HttpStatus>().unwrap(), HttpStatus::BadRequest);
        assert_eq!(
            "401".parse::<HttpStatus>().unwrap(),
            HttpStatus::Unauthorized
        );
        assert_eq!(
            "402".parse::<HttpStatus>().unwrap(),
            HttpStatus::PaymentRequired
        );
        assert_eq!("403".parse::<HttpStatus>().unwrap(), HttpStatus::Forbidden);
        assert_eq!("404".parse::<HttpStatus>().unwrap(), HttpStatus::NotFound);
        assert_eq!(
            "405".parse::<HttpStatus>().unwrap(),
            HttpStatus::MethodNotAllowed
        );
        assert_eq!(
            "406".parse::<HttpStatus>().unwrap(),
            HttpStatus::NotAcceptable
        );
        assert_eq!(
            "407".parse::<HttpStatus>().unwrap(),
            HttpStatus::ProxyAuthenticationRequired
        );
        assert_eq!(
            "408".parse::<HttpStatus>().unwrap(),
            HttpStatus::RequestTimeout
        );
        assert_eq!("409".parse::<HttpStatus>().unwrap(), HttpStatus::Conflict);
        assert_eq!("410".parse::<HttpStatus>().unwrap(), HttpStatus::Gone);
        assert_eq!(
            "411".parse::<HttpStatus>().unwrap(),
            HttpStatus::LengthRequired
        );
        assert_eq!(
            "412".parse::<HttpStatus>().unwrap(),
            HttpStatus::PreconditionFailed
        );
        assert_eq!(
            "413".parse::<HttpStatus>().unwrap(),
            HttpStatus::RequestEntityTooLarge
        );
        assert_eq!(
            "414".parse::<HttpStatus>().unwrap(),
            HttpStatus::RequestUriTooLong
        );
        assert_eq!(
            "415".parse::<HttpStatus>().unwrap(),
            HttpStatus::UnsupportedMediaType
        );
        assert_eq!(
            "416".parse::<HttpStatus>().unwrap(),
            HttpStatus::RequestedRangeNotSatisfiable
        );
        assert_eq!(
            "417".parse::<HttpStatus>().unwrap(),
            HttpStatus::ExpectationFailed
        );
        assert_eq!(
            "500".parse::<HttpStatus>().unwrap(),
            HttpStatus::InternalServerError
        );
        assert_eq!(
            "501".parse::<HttpStatus>().unwrap(),
            HttpStatus::NotImplemented
        );
        assert_eq!("502".parse::<HttpStatus>().unwrap(), HttpStatus::BadGateway);
        assert_eq!(
            "503".parse::<HttpStatus>().unwrap(),
            HttpStatus::ServiceUnavailable
        );
        assert_eq!(
            "504".parse::<HttpStatus>().unwrap(),
            HttpStatus::GatewayTimeout
        );
        assert_eq!(
            "505".parse::<HttpStatus>().unwrap(),
            HttpStatus::HttpVersionNotSupported
        );
        assert_eq!("200 OK".parse::<HttpStatus>().unwrap(), HttpStatus::OK);
        assert_eq!(
            "899".parse::<HttpStatus>().unwrap(),
            HttpStatus::Unknown(899)
        );
    }

    #[test]
    fn parse_error() {
        assert!("abc".parse::<HttpStatus>().is_err());
        assert!("301a".parse::<HttpStatus>().is_err());
    }

    #[test]
    fn display() {
        assert_eq!(&HttpStatus::Accepted.to_string(), "202 Accepted");
        assert_eq!(&HttpStatus::BadGateway.to_string(), "502 Bad Gateway");
        assert_eq!(&HttpStatus::BadRequest.to_string(), "400 Bad Request");
        assert_eq!(&HttpStatus::Conflict.to_string(), "409 Conflict");
        assert_eq!(&HttpStatus::Continue.to_string(), "100 Continue");
        assert_eq!(&HttpStatus::Created.to_string(), "201 Created");
        assert_eq!(
            &HttpStatus::ExpectationFailed.to_string(),
            "417 Expectation Failed"
        );
        assert_eq!(&HttpStatus::Forbidden.to_string(), "403 Forbidden");
        assert_eq!(&HttpStatus::Found.to_string(), "302 Found");
        assert_eq!(
            &HttpStatus::GatewayTimeout.to_string(),
            "504 Gateway Timeout"
        );
        assert_eq!(&HttpStatus::Gone.to_string(), "410 Gone");
        assert_eq!(
            &HttpStatus::HttpVersionNotSupported.to_string(),
            "505 HTTP Version Not Supported"
        );
        assert_eq!(
            &HttpStatus::InternalServerError.to_string(),
            "500 Internal Server Error"
        );
        assert_eq!(
            &HttpStatus::LengthRequired.to_string(),
            "411 Length Required"
        );
        assert_eq!(
            &HttpStatus::MethodNotAllowed.to_string(),
            "405 Method Not Allowed"
        );
        assert_eq!(
            &HttpStatus::MovedPermanently.to_string(),
            "301 Moved Permanently"
        );
        assert_eq!(
            &HttpStatus::MultipleChoices.to_string(),
            "300 Multiple Choices"
        );
        assert_eq!(&HttpStatus::NoContent.to_string(), "204 No Content");
        assert_eq!(
            &HttpStatus::NonAuthoritativeInformation.to_string(),
            "203 No Authoritative Information"
        );
        assert_eq!(&HttpStatus::NotAcceptable.to_string(), "406 Not Acceptable");
        assert_eq!(&HttpStatus::NotFound.to_string(), "404 Not Found");
        assert_eq!(
            &HttpStatus::NotImplemented.to_string(),
            "501 Not Implemented"
        );
        assert_eq!(&HttpStatus::NotModified.to_string(), "304 NotModified");
        assert_eq!(&HttpStatus::OK.to_string(), "200 OK");
        assert_eq!(
            &HttpStatus::PartialContent.to_string(),
            "206 Partial Content"
        );
        assert_eq!(
            &HttpStatus::PaymentRequired.to_string(),
            "402 Payment Required"
        );
        assert_eq!(
            &HttpStatus::PreconditionFailed.to_string(),
            "412 Precondition Failed"
        );
        assert_eq!(
            &HttpStatus::ProxyAuthenticationRequired.to_string(),
            "407 Prozy Authentication Required",
        );
        assert_eq!(
            &HttpStatus::RequestEntityTooLarge.to_string(),
            "413 Request Entity Too Large"
        );
        assert_eq!(
            &HttpStatus::RequestTimeout.to_string(),
            "408 Request Timeout"
        );
        assert_eq!(
            &HttpStatus::RequestUriTooLong.to_string(),
            "414 Request URI Too Long"
        );
        assert_eq!(
            &HttpStatus::RequestedRangeNotSatisfiable.to_string(),
            "416 Requested Range Not Satisfiable"
        );
        assert_eq!(&HttpStatus::ResetContent.to_string(), "205 Reset Content");
        assert_eq!(&HttpStatus::SeeOther.to_string(), "303 See Other");
        assert_eq!(
            &HttpStatus::ServiceUnavailable.to_string(),
            "503 Service Unavailable"
        );
        assert_eq!(
            &HttpStatus::SwitchingProtocols.to_string(),
            "101 Switching Protocols"
        );
        assert_eq!(
            &HttpStatus::TemporaryRedirect.to_string(),
            "307 Temporary Redirect"
        );
        assert_eq!(&HttpStatus::Unauthorized.to_string(), "401 Unauthorized");
        assert_eq!(
            &HttpStatus::UnsupportedMediaType.to_string(),
            "415 Unsupported Media Type"
        );
        assert_eq!(&HttpStatus::UseProxy.to_string(), "305 Use Proxy");
        assert_eq!(&HttpStatus::Unknown(899).to_string(), "899");
    }

    #[test]
    fn parse_display_round_trip() {
        assert_eq!(
            "202 Accepted".parse::<HttpStatus>().unwrap().to_string(),
            "202 Accepted"
        );
        assert_eq!(
            "502 Bad Gateway".parse::<HttpStatus>().unwrap().to_string(),
            "502 Bad Gateway"
        );
        assert_eq!(
            "400 Bad Request".parse::<HttpStatus>().unwrap().to_string(),
            "400 Bad Request"
        );
        assert_eq!(
            "409 Conflict".parse::<HttpStatus>().unwrap().to_string(),
            "409 Conflict"
        );
        assert_eq!(
            "100 Continue".parse::<HttpStatus>().unwrap().to_string(),
            "100 Continue"
        );
        assert_eq!(
            "201 Created".parse::<HttpStatus>().unwrap().to_string(),
            "201 Created"
        );
        assert_eq!(
            "417 Expectation Failed"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "417 Expectation Failed"
        );
        assert_eq!(
            "403 Forbidden".parse::<HttpStatus>().unwrap().to_string(),
            "403 Forbidden"
        );
        assert_eq!(
            "302 Found".parse::<HttpStatus>().unwrap().to_string(),
            "302 Found"
        );
        assert_eq!(
            "504 Gateway Timeout"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "504 Gateway Timeout"
        );
        assert_eq!(
            "410 Gone".parse::<HttpStatus>().unwrap().to_string(),
            "410 Gone"
        );
        assert_eq!(
            "505 HTTP Version Not Supported"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "505 HTTP Version Not Supported"
        );
        assert_eq!(
            "500 Internal Server Error"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "500 Internal Server Error"
        );
        assert_eq!(
            "411 Length Required"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "411 Length Required"
        );
        assert_eq!(
            "405 Method Not Allowed"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "405 Method Not Allowed"
        );
        assert_eq!(
            "301 Moved Permanently"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "301 Moved Permanently"
        );
        assert_eq!(
            "300 Multiple Choices"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "300 Multiple Choices"
        );
        assert_eq!(
            "204 No Content".parse::<HttpStatus>().unwrap().to_string(),
            "204 No Content"
        );
        assert_eq!(
            "203 No Authoritative Information"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "203 No Authoritative Information"
        );
        assert_eq!(
            "406 Not Acceptable"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "406 Not Acceptable"
        );
        assert_eq!(
            "404 Not Found".parse::<HttpStatus>().unwrap().to_string(),
            "404 Not Found"
        );
        assert_eq!(
            "501 Not Implemented"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "501 Not Implemented"
        );
        assert_eq!(
            "304 NotModified".parse::<HttpStatus>().unwrap().to_string(),
            "304 NotModified"
        );
        assert_eq!(
            "200 OK".parse::<HttpStatus>().unwrap().to_string(),
            "200 OK"
        );
        assert_eq!(
            "206 Partial Content"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "206 Partial Content"
        );
        assert_eq!(
            "402 Payment Required"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "402 Payment Required"
        );
        assert_eq!(
            "412 Precondition Failed"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "412 Precondition Failed"
        );
        assert_eq!(
            "407 Prozy Authentication Required"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "407 Prozy Authentication Required"
        );
        assert_eq!(
            "413 Request Entity Too Large"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "413 Request Entity Too Large"
        );
        assert_eq!(
            "408 Request Timeout"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "408 Request Timeout"
        );
        assert_eq!(
            "414 Request URI Too Long"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "414 Request URI Too Long"
        );
        assert_eq!(
            "416 Requested Range Not Satisfiable"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "416 Requested Range Not Satisfiable"
        );
        assert_eq!(
            "205 Reset Content"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "205 Reset Content"
        );
        assert_eq!(
            "303 See Other".parse::<HttpStatus>().unwrap().to_string(),
            "303 See Other"
        );
        assert_eq!(
            "503 Service Unavailable"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "503 Service Unavailable"
        );
        assert_eq!(
            "101 Switching Protocols"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "101 Switching Protocols"
        );
        assert_eq!(
            "307 Temporary Redirect"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "307 Temporary Redirect"
        );
        assert_eq!(
            "401 Unauthorized"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "401 Unauthorized"
        );
        assert_eq!(
            "415 Unsupported Media Type"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "415 Unsupported Media Type"
        );
        assert_eq!(
            "305 Use Proxy".parse::<HttpStatus>().unwrap().to_string(),
            "305 Use Proxy"
        );
        assert_eq!(&"889".parse::<HttpStatus>().unwrap().to_string(), "889");
    }
}

#[derive(Debug, PartialEq, Eq)]
struct HttpHeader {
    key: String,
    value: String,
}

impl HttpHeader {
    fn new(key: impl AsRef<str>, value: impl Into<String>) -> Self {
        HttpHeader {
            key: key.as_ref().to_lowercase(),
            value: value.into(),
        }
    }

    fn deserialize(s: &str) -> Result<Self> {
        let mut parser = Parser::new(s);
        let key = parser.parse_until(":")?;
        parser.expect(": ")?;
        let value = parser.parse_remaining()?;

        Ok(HttpHeader::new(key.to_lowercase(), value))
    }
}

#[cfg(test)]
mod http_header_tests {
    use super::HttpHeader;

    #[test]
    fn parse_success() {
        assert_eq!(
            HttpHeader::deserialize("key: value").unwrap(),
            HttpHeader::new("key", "value")
        );
        assert_eq!(
            HttpHeader::deserialize("key: value1 value2").unwrap(),
            HttpHeader::new("key", "value1 value2")
        );
    }

    #[test]
    fn parse_failure_no_value() {
        assert!(HttpHeader::deserialize("key").is_err());
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct HttpHeaders {
    headers: BTreeMap<String, String>,
}

#[macro_export]
macro_rules! http_headers {
    ($($key:expr => $value:expr),* ,) => (
        $crate::hash_map!($($key => $value),*)
    );
    ($($key:expr => $value:expr),*) => ({
        ::core::iter::Iterator::collect::<$crate::protocol::HttpHeaders>(
            ::core::iter::IntoIterator::into_iter([
                $((
                    ::core::convert::From::from($key),
                    ::core::convert::From::from($value)
                ),)*
            ])
        )
    });
}

impl HttpHeaders {
    fn new() -> Self {
        HttpHeaders {
            headers: BTreeMap::new(),
        }
    }

    pub fn get(&self, key: impl AsRef<str>) -> Option<&str> {
        self.headers
            .get(&key.as_ref().to_lowercase())
            .map(convert::AsRef::as_ref)
    }

    pub fn insert(&mut self, key: impl AsRef<str>, value: impl Into<String>) {
        self.headers
            .insert(key.as_ref().to_lowercase(), value.into());
    }

    fn deserialize<R: io::Read>(s: &mut CrLfStream<R>) -> Result<Self> {
        let mut headers = vec![];
        let mut iter = s.peekable();
        while let Some(line) = iter.next() {
            let mut line = line?;
            while let Some(Ok(next_line)) = iter.peek() {
                if !next_line.starts_with(' ') && !next_line.starts_with('\t') {
                    break;
                }
                line.push_str(&iter.next().unwrap()?);
            }
            headers.push(HttpHeader::deserialize(&line)?);
        }
        Ok(HttpHeaders::from(headers))
    }

    fn serialize<W: io::Write>(&self, mut w: W) -> Result<()> {
        for (key, value) in &self.headers {
            write!(&mut w, "{}: {}\r\n", key, value)?;
        }
        Ok(())
    }
}

impl iter::FromIterator<(String, String)> for HttpHeaders {
    fn from_iter<T: IntoIterator<Item = (String, String)>>(iter: T) -> Self {
        Self {
            headers: iter.into_iter().collect(),
        }
    }
}

impl<'a> IntoIterator for &'a HttpHeaders {
    type Item = (&'a String, &'a String);
    type IntoIter = BTreeMapIter<'a, String, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.headers.iter()
    }
}

#[test]
fn http_headers_case_insensitive() {
    for k1 in ["FOO", "FoO", "foo"] {
        let mut headers = HttpHeaders::new();
        headers.insert(k1, "Bar");
        headers.insert(String::from(k1), "Bar");

        for k2 in ["FOO", "FoO", "foo"] {
            assert_eq!(headers.get(k2), Some("Bar"));
            assert_eq!(headers.get(String::from(k2)), Some("Bar"));
        }
    }
}

impl From<Vec<HttpHeader>> for HttpHeaders {
    fn from(mut headers: Vec<HttpHeader>) -> Self {
        let mut map = BTreeMap::new();
        for h in headers.drain(..) {
            map.insert(h.key, h.value);
        }
        HttpHeaders { headers: map }
    }
}

#[cfg(test)]
mod http_headers_tests {
    use super::{CrLfStream, HttpHeader, HttpHeaders};
    use std::str;

    #[test]
    fn to_string() {
        let headers = HttpHeaders::from(vec![HttpHeader::new("A", "B"), HttpHeader::new("c", "d")]);
        let mut data = Vec::new();
        headers.serialize(&mut data).unwrap();
        assert_eq!(str::from_utf8(&data).unwrap(), "a: B\r\nc: d\r\n");
    }

    #[test]
    fn serialize_empty() {
        let headers = HttpHeaders::from(vec![]);
        let mut data = Vec::new();
        headers.serialize(&mut data).unwrap();
        assert_eq!(str::from_utf8(&data).unwrap(), "");
    }

    #[test]
    fn deserialize_success() {
        let mut input = CrLfStream::new("A: b\r\nC: d\r\n\r\n".as_bytes());
        let actual = HttpHeaders::deserialize(&mut input).unwrap();
        let expected =
            HttpHeaders::from(vec![HttpHeader::new("a", "b"), HttpHeader::new("c", "d")]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn deserialize_success_header_continuation() {
        let mut input = CrLfStream::new("a: b\r\n e\r\nc: d\r\n\r\n".as_bytes());
        let actual = HttpHeaders::deserialize(&mut input).unwrap();
        let expected =
            HttpHeaders::from(vec![HttpHeader::new("a", "b e"), HttpHeader::new("c", "d")]);
        assert_eq!(actual, expected);
    }
}

pub struct HttpResponse<B: io::Read> {
    version: HttpVersion,
    pub status: HttpStatus,
    pub headers: HttpHeaders,
    pub body: HttpBody<B>,
}

impl HttpResponse<Box<dyn io::Read>> {
    pub fn from_string<S: Into<String>>(status: HttpStatus, s: S) -> Self {
        HttpResponse::new(status, Box::new(io::Cursor::new(s.into())))
    }
}

impl<B: io::Read> HttpResponse<B> {
    pub fn new(status: HttpStatus, body: B) -> Self {
        let body = HttpBody::ReadTilClose(io::BufReader::new(body));
        HttpResponse {
            version: HttpVersion::new(1, 1),
            status,
            headers: HttpHeaders::new(),
            body,
        }
    }

    pub fn deserialize(mut socket: B) -> Result<Self> {
        let mut s = CrLfStream::new(&mut socket);
        let first_line = s.expect_next()?;
        let mut parser = Parser::new(&first_line);

        let version = parser.parse_token()?.parse()?;
        let status = parser.parse_remaining()?.parse()?;

        let headers = HttpHeaders::deserialize(&mut s)?;
        drop(s);

        let encoding = headers.get("Transfer-Encoding");
        let content_length = headers.get("Content-Length").map(str::parse).transpose()?;

        let body = HttpBody::new(encoding, content_length, io::BufReader::new(socket));

        Ok(HttpResponse {
            version,
            status,
            headers,
            body,
        })
    }

    pub fn get_header(&self, key: &str) -> Option<&str> {
        self.headers.get(key)
    }

    pub fn add_header(&mut self, key: impl AsRef<str>, value: impl Into<String>) {
        self.headers.insert(key, value);
    }

    pub fn serialize<W: io::Write>(&self, mut w: W) -> Result<()> {
        write!(&mut w, "{} {}\r\n", self.version, self.status)?;
        self.headers.serialize(&mut w)?;
        write!(&mut w, "\r\n")?;
        Ok(())
    }
}

#[cfg(test)]
mod http_response_tests {
    use super::{HttpResponse, HttpStatus};
    use std::io;

    #[test]
    fn parse_success() {
        let input = "HTTP/1.1 200 OK\r\nA: B\r\nC: D\r\n\r\n".as_bytes();
        let actual = HttpResponse::deserialize(input).unwrap();
        let mut expected = HttpResponse::new(HttpStatus::OK, io::empty());
        expected.add_header("A", "B");
        expected.add_header("C", "D");
        assert_eq!(actual.version, expected.version);
        assert_eq!(actual.status, expected.status);
        assert_eq!(actual.headers, expected.headers);
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum HttpMethod {
    Delete,
    Get,
    Head,
    Options,
    Post,
    Put,
    Trace,
}

impl str::FromStr for HttpMethod {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_uppercase().as_ref() {
            "DELETE" => Ok(HttpMethod::Delete),
            "GET" => Ok(HttpMethod::Get),
            "HEAD" => Ok(HttpMethod::Head),
            "OPTIONS" => Ok(HttpMethod::Options),
            "POST" => Ok(HttpMethod::Post),
            "PUT" => Ok(HttpMethod::Put),
            "TRACE" => Ok(HttpMethod::Trace),
            m => Err(Error::ParseError(format!("Unknown method {}", m))),
        }
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HttpMethod::Delete => write!(f, "DELETE"),
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Head => write!(f, "HEAD"),
            HttpMethod::Options => write!(f, "OPTIONS"),
            HttpMethod::Post => write!(f, "POST"),
            HttpMethod::Put => write!(f, "PUT"),
            HttpMethod::Trace => write!(f, "TRACE"),
        }
    }
}

impl HttpMethod {
    pub fn has_body(&self) -> bool {
        match self {
            Self::Delete | Self::Post | Self::Put => true,
            Self::Trace | Self::Get | Self::Head | Self::Options => false,
        }
    }
}

#[cfg(test)]
mod http_method_tests {
    use super::HttpMethod;
    use std::string::ToString;

    #[test]
    fn parse_success() {
        assert_eq!("DELETE".parse::<HttpMethod>().unwrap(), HttpMethod::Delete);
        assert_eq!("GET".parse::<HttpMethod>().unwrap(), HttpMethod::Get);
        assert_eq!("HEAD".parse::<HttpMethod>().unwrap(), HttpMethod::Head);
        assert_eq!(
            "OPTIONS".parse::<HttpMethod>().unwrap(),
            HttpMethod::Options
        );
        assert_eq!("POST".parse::<HttpMethod>().unwrap(), HttpMethod::Post);
        assert_eq!("PUT".parse::<HttpMethod>().unwrap(), HttpMethod::Put);
        assert_eq!("TRACE".parse::<HttpMethod>().unwrap(), HttpMethod::Trace);
    }

    #[test]
    fn parse_error() {
        assert!("GE".parse::<HttpMethod>().is_err());
        assert!("BLARG".parse::<HttpMethod>().is_err());
    }

    #[test]
    fn display() {
        assert_eq!(&HttpMethod::Delete.to_string(), "DELETE");
        assert_eq!(&HttpMethod::Get.to_string(), "GET");
        assert_eq!(&HttpMethod::Head.to_string(), "HEAD");
        assert_eq!(&HttpMethod::Options.to_string(), "OPTIONS");
        assert_eq!(&HttpMethod::Post.to_string(), "POST");
        assert_eq!(&HttpMethod::Put.to_string(), "PUT");
        assert_eq!(&HttpMethod::Trace.to_string(), "TRACE");
    }

    #[test]
    fn parse_display_round_trip() {
        assert_eq!(
            &"DELETE".parse::<HttpMethod>().unwrap().to_string(),
            "DELETE"
        );
        assert_eq!(&"GET".parse::<HttpMethod>().unwrap().to_string(), "GET");
        assert_eq!(&"HEAD".parse::<HttpMethod>().unwrap().to_string(), "HEAD");
        assert_eq!(&"POST".parse::<HttpMethod>().unwrap().to_string(), "POST");
        assert_eq!(
            &"OPTIONS".parse::<HttpMethod>().unwrap().to_string(),
            "OPTIONS"
        );
        assert_eq!(&"PUT".parse::<HttpMethod>().unwrap().to_string(), "PUT");
        assert_eq!(&"TRACE".parse::<HttpMethod>().unwrap().to_string(), "TRACE");
    }
}

pub struct HttpRequest<B: io::Read> {
    pub method: HttpMethod,
    pub uri: String,
    version: HttpVersion,
    pub headers: HttpHeaders,
    pub body: HttpBody<B>,
}

impl HttpRequest<io::Empty> {
    pub fn new<S: Into<String>>(method: HttpMethod, uri_in: S) -> Self {
        let uri_in = uri_in.into();
        let uri = if uri_in.is_empty() {
            "/".into()
        } else {
            uri_in
        };

        HttpRequest {
            method,
            uri,
            version: HttpVersion::new(1, 1),
            headers: HttpHeaders::new(),
            body: HttpBody::ReadTilClose(io::BufReader::new(io::empty())),
        }
    }
}

pub enum OutgoingRequest<S: io::Read + io::Write> {
    NoBody(S),
    WithBody(OutgoingBody<S>),
}

impl<S: io::Read + io::Write> OutgoingRequest<S> {
    fn with_body(socket: io::BufWriter<S>) -> Self {
        Self::WithBody(OutgoingBody::new(socket))
    }

    fn with_no_body(socket: S) -> Self {
        Self::NoBody(socket)
    }

    pub fn finish(self) -> Result<HttpResponse<S>> {
        match self {
            Self::NoBody(mut socket) => {
                socket.flush()?;
                Ok(HttpResponse::deserialize(socket)?)
            }
            Self::WithBody(body) => body.finish(),
        }
    }
}

impl<S: io::Read + io::Write> io::Write for OutgoingRequest<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            #[cfg(feature = "std")]
            Self::NoBody(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Method does not support a body"),
            )),
            #[cfg(not(feature = "std"))]
            Self::NoBody(_) => Err(Error::Other(format!("Method does not support a body"))),
            Self::WithBody(b) => b.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::WithBody(b) => b.flush(),
            _ => Ok(()),
        }
    }
}

pub struct OutgoingBody<S: io::Read + io::Write> {
    socket: io::BufWriter<S>,
}

impl<S: io::Read + io::Write> io::Write for OutgoingBody<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = buf.len();
        if len == 0 {
            return Ok(0);
        }
        write!(&mut self.socket, "{:x}\r\n", len)?;
        self.socket.write_all(buf)?;
        write!(&mut self.socket, "\r\n")?;
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}

impl<S: io::Read + io::Write> OutgoingBody<S> {
    fn new(socket: io::BufWriter<S>) -> Self {
        OutgoingBody { socket }
    }

    pub fn finish(mut self) -> Result<HttpResponse<S>> {
        write!(&mut self.socket, "0\r\n\r\n")?;
        self.socket.flush()?;

        let socket = self.socket.into_inner()?;
        Ok(HttpResponse::deserialize(socket)?)
    }
}

impl<B: io::Read> HttpRequest<B> {
    pub fn add_header(&mut self, key: impl AsRef<str>, value: impl Into<String>) {
        self.headers.insert(key, value);
    }

    pub fn deserialize(mut stream: io::BufReader<B>) -> Result<Self> {
        let mut ts = CrLfStream::new(&mut stream);
        let first_line = ts.expect_next()?;
        let mut parser = Parser::new(&first_line);

        let method = parser.parse_token()?.parse()?;
        let uri = parser.parse_token()?.into();
        let version = parser.parse_token()?.parse()?;
        let headers = HttpHeaders::deserialize(&mut ts)?;
        drop(ts);

        let encoding = headers.get("Transfer-Encoding");
        let content_length = headers.get("Content-Length").map(str::parse).transpose()?;
        let body = HttpBody::new(encoding, content_length, stream);

        Ok(HttpRequest {
            method,
            uri,
            version,
            headers,
            body,
        })
    }
}

impl<B: io::Read> HttpRequest<B> {
    pub fn serialize<S: io::Read + io::Write>(
        &self,
        mut w: io::BufWriter<S>,
    ) -> Result<OutgoingRequest<S>> {
        write!(&mut w, "{} {} {}\r\n", self.method, self.uri, self.version)?;
        self.headers.serialize(&mut w)?;
        write!(&mut w, "\r\n")?;
        if self.method.has_body() {
            Ok(OutgoingRequest::with_body(w))
        } else {
            Ok(OutgoingRequest::with_no_body(w.into_inner()?))
        }
    }
}

#[cfg(test)]
mod http_request_tests {
    use super::{HttpMethod, HttpRequest};
    use std::io;

    #[test]
    fn parse_success() {
        let mut input = "GET /a/b HTTP/1.1\r\nA: B\r\nC: D\r\n\r\n".as_bytes();
        let actual = HttpRequest::deserialize(io::BufReader::new(&mut input)).unwrap();
        let mut expected = HttpRequest::new(HttpMethod::Get, "/a/b");
        expected.add_header("A", "B");
        expected.add_header("C", "D");
        assert_eq!(actual.version, expected.version);
        assert_eq!(actual.method, expected.method);
        assert_eq!(actual.headers, expected.headers);
    }
}
