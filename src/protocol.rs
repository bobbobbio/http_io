//! Types representing various parts of the HTTP protocol.

// We do write! + '\r\n' and don't want to hide the line ending in a writeln!
#![allow(clippy::write_with_newline)]

use crate::error::{Error, Result};
use crate::io::{self, Read, Write};
#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, collections::BTreeMap, format, string::String, vec, vec::Vec};
use core::cmp;
use core::convert;
use core::fmt;
use core::str;
#[cfg(feature = "std")]
use std::collections::BTreeMap;

struct HttpBodyChunk<S: io::Read> {
    inner: io::Take<HttpReadTilCloseBody<S>>,
}

pub struct HttpChunkedBody<S: io::Read> {
    stream: Option<HttpReadTilCloseBody<S>>,
    chunk: Option<HttpBodyChunk<S>>,
}

impl<S: io::Read> HttpChunkedBody<S> {
    fn new(stream: HttpReadTilCloseBody<S>) -> Self {
        HttpChunkedBody {
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
        let mut body = HttpChunkedBody::new(input);

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
            HttpBody::Chunked(HttpChunkedBody::new(body))
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
    MovedPermanently,
    OK,
    LengthRequired,
    InternalServerError,
    MethodNotAllowed,
    Unknown(u32),
}

impl str::FromStr for HttpStatus {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let mut parser = Parser::new(s);
        match parser.parse_number()? {
            200 => Ok(HttpStatus::OK),
            301 => Ok(HttpStatus::MovedPermanently),
            405 => Ok(HttpStatus::MethodNotAllowed),
            411 => Ok(HttpStatus::LengthRequired),
            500 => Ok(HttpStatus::InternalServerError),
            v => Ok(HttpStatus::Unknown(v)),
        }
    }
}

impl fmt::Display for HttpStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HttpStatus::OK => write!(f, "200 OK"),
            HttpStatus::MovedPermanently => write!(f, "301 Moved Permanently"),
            HttpStatus::MethodNotAllowed => write!(f, "405 Method Not Allowed"),
            HttpStatus::LengthRequired => write!(f, "411 Length Required"),
            HttpStatus::InternalServerError => write!(f, "500 Internal Server Error"),
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
        assert_eq!(
            "301".parse::<HttpStatus>().unwrap(),
            HttpStatus::MovedPermanently
        );
        assert_eq!(
            "405".parse::<HttpStatus>().unwrap(),
            HttpStatus::MethodNotAllowed
        );
        assert_eq!(
            "411".parse::<HttpStatus>().unwrap(),
            HttpStatus::LengthRequired
        );
        assert_eq!(
            "500".parse::<HttpStatus>().unwrap(),
            HttpStatus::InternalServerError
        );
        assert_eq!("200 OK".parse::<HttpStatus>().unwrap(), HttpStatus::OK);
        assert_eq!("200".parse::<HttpStatus>().unwrap(), HttpStatus::OK);
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
        assert_eq!(
            &HttpStatus::MovedPermanently.to_string(),
            "301 Moved Permanently"
        );
        assert_eq!(&HttpStatus::OK.to_string(), "200 OK");
        assert_eq!(
            &HttpStatus::MethodNotAllowed.to_string(),
            "405 Method Not Allowed"
        );
        assert_eq!(
            &HttpStatus::LengthRequired.to_string(),
            "411 Length Required"
        );
        assert_eq!(
            &HttpStatus::InternalServerError.to_string(),
            "500 Internal Server Error"
        );
        assert_eq!(&HttpStatus::Unknown(899).to_string(), "899");
    }

    #[test]
    fn parse_display_round_trip() {
        assert_eq!(
            &"301 Moved Permanently"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "301 Moved Permanently"
        );
        assert_eq!(
            &"200 OK".parse::<HttpStatus>().unwrap().to_string(),
            "200 OK"
        );
        assert_eq!(
            &"405 Method Not Allowed"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "405 Method Not Allowed"
        );
        assert_eq!(
            &"411 Length Required"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "411 Length Required"
        );
        assert_eq!(
            &"500 Internal Server Error"
                .parse::<HttpStatus>()
                .unwrap()
                .to_string(),
            "500 Internal Server Error"
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
    fn new<K: Into<String>, V: Into<String>>(key: K, value: V) -> Self {
        HttpHeader {
            key: key.into(),
            value: value.into(),
        }
    }

    fn deserialize(s: &str) -> Result<Self> {
        let mut parser = Parser::new(s);
        let key = parser.parse_until(":")?;
        parser.expect(": ")?;
        let value = parser.parse_remaining()?;

        Ok(HttpHeader::new(key, value))
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

#[derive(Debug, PartialEq, Eq)]
pub struct HttpHeaders {
    headers: BTreeMap<String, String>,
}

impl HttpHeaders {
    fn new() -> Self {
        HttpHeaders {
            headers: BTreeMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(key).map(convert::AsRef::as_ref)
    }

    pub fn insert<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
        self.headers.insert(key.into(), value.into());
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
        let headers = HttpHeaders::from(vec![HttpHeader::new("a", "b"), HttpHeader::new("c", "d")]);
        let mut data = Vec::new();
        headers.serialize(&mut data).unwrap();
        assert_eq!(str::from_utf8(&data).unwrap(), "a: b\r\nc: d\r\n");
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
        let mut input = CrLfStream::new("a: b\r\nc: d\r\n\r\n".as_bytes());
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

    #[cfg(test)]
    pub fn add_header<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
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
    pub fn new<S: Into<String>>(method: HttpMethod, uri: S) -> Self {
        HttpRequest {
            method,
            uri: uri.into(),
            version: HttpVersion::new(1, 1),
            headers: HttpHeaders::new(),
            body: HttpBody::ReadTilClose(io::BufReader::new(io::empty())),
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
    pub fn add_header<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
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
    ) -> Result<OutgoingBody<S>> {
        write!(&mut w, "{} {} {}\r\n", self.method, self.uri, self.version)?;
        self.headers.serialize(&mut w)?;
        write!(&mut w, "\r\n")?;
        Ok(OutgoingBody::new(w))
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
