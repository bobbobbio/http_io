use crate::error::{Error, Result};
use std::cmp;
use std::collections::BTreeMap;
use std::convert;
use std::fmt;
use std::io;
use std::str;

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
            Ok(v) => v.map(|i| Ok(i)),
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
                if before.len() == 0 {
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
            .ok_or(Error::UnexpectedEof("Expected line".into()))
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

struct Parser<'a> {
    s: &'a str,
    position: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Parser { s, position: 0 }
    }

    fn expect(&mut self, expected: &str) -> Result<()> {
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

    fn parse_digit(&mut self) -> Result<u32> {
        if self.position >= self.s.len() {
            return Err(Error::UnexpectedEof(format!("Expected digit")));
        }

        let digit = &self.s[self.position..(self.position + 1)];
        self.position += 1;
        Ok(digit.parse()?)
    }

    fn parse_until(&mut self, div: &str) -> Result<&'a str> {
        if self.position >= self.s.len() {
            return Err(Error::UnexpectedEof(format!("Expected '{}'", div)));
        }

        let remaining = &self.s[self.position..];
        let pos = remaining.find(div).ok_or(Error::ParseError(format!(
            "Expected '{}' in '{}'",
            div, remaining
        )))?;
        self.position += pos;
        Ok(&remaining[..pos])
    }

    fn consume_whilespace(&mut self) {
        while self.position < self.s.len()
            && (self.s[self.position..].starts_with(" ")
                || self.s[self.position..].starts_with("\t"))
        {
            self.position += 1
        }
    }

    fn parse_token(&mut self) -> Result<&'a str> {
        if self.position >= self.s.len() {
            return Err(Error::UnexpectedEof("Expected token".into()));
        }

        let remaining = &self.s[self.position..];
        let token = remaining.split(|c| c == ' ' || c == '\t').next().unwrap();
        self.position += token.len();
        self.consume_whilespace();

        Ok(token)
    }

    fn parse_number(&mut self) -> Result<u32> {
        Ok(self.parse_token()?.parse()?)
    }

    fn parse_remaining(&mut self) -> Result<&str> {
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
    Unknown(u32),
}

impl str::FromStr for HttpStatus {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let mut parser = Parser::new(s);
        match parser.parse_number()? {
            301 => Ok(HttpStatus::MovedPermanently),
            200 => Ok(HttpStatus::OK),
            v => Ok(HttpStatus::Unknown(v)),
        }
    }
}

impl fmt::Display for HttpStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HttpStatus::OK => write!(f, "200 OK"),
            HttpStatus::MovedPermanently => write!(f, "301 Moved Permanently"),
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
}

impl str::FromStr for HttpHeader {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
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
            "key: value".parse::<HttpHeader>().unwrap(),
            HttpHeader::new("key", "value")
        );
        assert_eq!(
            "key: value1 value2".parse::<HttpHeader>().unwrap(),
            HttpHeader::new("key", "value1 value2")
        );
    }

    #[test]
    fn parse_failure_no_value() {
        assert!("key".parse::<HttpHeader>().is_err());
    }
}

#[derive(Debug, PartialEq, Eq)]
struct HttpHeaders {
    headers: BTreeMap<String, String>,
}

impl HttpHeaders {
    fn new() -> Self {
        HttpHeaders {
            headers: BTreeMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.headers.get(key.into()).map(convert::AsRef::as_ref)
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
                if !next_line.starts_with(" ") && !next_line.starts_with("\t") {
                    break;
                }
                line.push_str(&iter.next().unwrap()?);
            }
            headers.push(line.parse()?);
        }
        Ok(HttpHeaders::from(headers))
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

impl fmt::Display for HttpHeaders {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (key, value) in &self.headers {
            write!(f, "{}: {}\r\n", key, value)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod http_headers_tests {
    use super::{CrLfStream, HttpHeader, HttpHeaders};

    #[test]
    fn to_string() {
        let headers = HttpHeaders::from(vec![HttpHeader::new("a", "b"), HttpHeader::new("c", "d")]);
        assert_eq!(&headers.to_string(), "a: b\r\nc: d\r\n");
    }

    #[test]
    fn to_string_empty() {
        let headers = HttpHeaders::from(vec![]);
        assert_eq!(&headers.to_string(), "");
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

#[derive(Debug, PartialEq, Eq)]
pub struct HttpResponse {
    version: HttpVersion,
    status: HttpStatus,
    headers: HttpHeaders,
}

impl HttpResponse {
    pub fn new(status: HttpStatus) -> Self {
        HttpResponse {
            version: HttpVersion::new(1, 1),
            status,
            headers: HttpHeaders::new(),
        }
    }

    pub fn status(&self) -> HttpStatus {
        self.status
    }

    pub fn deserialize<R: io::Read>(s: &mut CrLfStream<R>) -> Result<Self> {
        let first_line = s.expect_next()?;
        let mut parser = Parser::new(&first_line);

        let version = parser.parse_token()?.parse()?;
        let status = parser.parse_remaining()?.parse()?;

        let headers = HttpHeaders::deserialize(s)?;

        Ok(HttpResponse {
            version,
            status,
            headers,
        })
    }

    pub fn get_header(&self, key: &str) -> Option<&str> {
        self.headers.get(key)
    }

    #[cfg(test)]
    pub fn add_header<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
        self.headers.insert(key, value);
    }
}

impl fmt::Display for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}\r\n", self.version, self.status)?;
        write!(f, "{}", self.headers)?;
        write!(f, "\r\n")?;
        Ok(())
    }
}

#[cfg(test)]
mod http_response_tests {
    use super::{CrLfStream, HttpResponse, HttpStatus};

    #[test]
    fn parse_success() {
        let mut input = CrLfStream::new("HTTP/1.1 200 OK\r\nA: B\r\nC: D\r\n\r\n".as_bytes());
        let actual = HttpResponse::deserialize(&mut input).unwrap();
        let mut expected = HttpResponse::new(HttpStatus::OK);
        expected.add_header("A", "B");
        expected.add_header("C", "D");
        assert_eq!(actual, expected);
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum HttpMethod {
    Get,
}

impl str::FromStr for HttpMethod {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_uppercase().as_ref() {
            "GET" => Ok(HttpMethod::Get),
            m => Err(Error::ParseError(format!("Unknown method {}", m))),
        }
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "GET")
    }
}

#[cfg(test)]
mod http_method_tests {
    use super::HttpMethod;
    use std::string::ToString;

    #[test]
    fn parse_success() {
        assert_eq!("GET".parse::<HttpMethod>().unwrap(), HttpMethod::Get);
    }

    #[test]
    fn parse_error() {
        assert!("GE".parse::<HttpMethod>().is_err());
        assert!("BLARG".parse::<HttpMethod>().is_err());
    }

    #[test]
    fn display() {
        assert_eq!(&HttpMethod::Get.to_string(), "GET");
    }

    #[test]
    fn parse_display_round_trip() {
        assert_eq!(&"GET".parse::<HttpMethod>().unwrap().to_string(), "GET");
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct HttpRequest {
    method: HttpMethod,
    uri: String,
    version: HttpVersion,
    headers: HttpHeaders,
}

impl fmt::Display for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {} {}\r\n", self.method, self.uri, self.version)?;
        write!(f, "{}", self.headers)?;
        write!(f, "\r\n")?;
        Ok(())
    }
}

impl HttpRequest {
    pub fn new<S: Into<String>>(method: HttpMethod, uri: S) -> Self {
        HttpRequest {
            method,
            uri: uri.into(),
            version: HttpVersion::new(1, 1),
            headers: HttpHeaders::new(),
        }
    }

    pub fn add_header<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
        self.headers.insert(key, value);
    }

    pub fn deserialize<R: io::Read>(s: &mut CrLfStream<R>) -> Result<Self> {
        let first_line = s.expect_next()?;
        let mut parser = Parser::new(&first_line);

        let method = parser.parse_token()?.parse()?;
        let uri = parser.parse_token()?.into();
        let version = parser.parse_token()?.parse()?;
        let headers = HttpHeaders::deserialize(s)?;

        Ok(HttpRequest {
            method,
            uri,
            version,
            headers,
        })
    }
}

#[cfg(test)]
mod http_request_tests {
    use super::{CrLfStream, HttpMethod, HttpRequest};

    #[test]
    fn parse_success() {
        let mut input = CrLfStream::new("GET /a/b HTTP/1.1\r\nA: B\r\nC: D\r\n\r\n".as_bytes());
        let actual = HttpRequest::deserialize(&mut input).unwrap();
        let mut expected = HttpRequest::new(HttpMethod::Get, "/a/b");
        expected.add_header("A", "B");
        expected.add_header("C", "D");
        assert_eq!(actual, expected);
    }
}
