//! A RFC 3986 URL
//!
//! # Example
//! ```rust
//! use http_io::url;
//!
//! let url: url::Url = "http://user:pass@www.google.com:8080/foo?bar#baz".parse().unwrap();
//! assert_eq!(url.scheme, url::Scheme::Http);
//! assert_eq!(url.authority, "www.google.com");
//! assert_eq!(url.port, Some(8080));
//! assert_eq!(url.path, "/foo".parse().unwrap());
//! assert_eq!(url.query, Some("bar".into()));
//! assert_eq!(url.fragment, Some("baz".into()));
//! assert_eq!(url.user_information, Some("user:pass".into()));
//! ```
use crate::error::{Error, Result};
use crate::protocol::Parser;
use std::convert::TryFrom;
use std::fmt;
use std::str;

#[derive(PartialEq, Debug, Clone)]
pub enum Scheme {
    Http,
    Https,
    File,
    Other(String),
}

impl str::FromStr for Scheme {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match s.to_lowercase().as_ref() {
            "http" => Scheme::Http,
            "https" => Scheme::Https,
            "file" => Scheme::File,
            s => Scheme::Other(s.into()),
        })
    }
}

impl fmt::Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Scheme::Http => write!(f, "http"),
            Scheme::Https => write!(f, "https"),
            Scheme::File => write!(f, "file"),
            Scheme::Other(s) => write!(f, "{}", s),
        }
    }
}

fn percent_encode_char(c: char) -> String {
    c.to_string()
        .bytes()
        .map(|b| format!("%{:x}", b))
        .collect::<Vec<_>>()
        .join("")
}

fn is_unreserved_char(c: char) -> bool {
    (c >= 'a' && c <= 'z')
        || (c >= 'A' && c <= 'Z')
        || (c >= '0' && c <= '9')
        || c == '-'
        || c == '.'
        || c == '_'
        || c == '~'
}

fn percent_encode(s: &str) -> String {
    s.chars()
        .map(|c| {
            if is_unreserved_char(c) {
                c.to_string()
            } else {
                percent_encode_char(c)
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

#[test]
fn percent_encode_unreserved_chars_not_encoded() {
    let s = "abcdefghijklmnopqrstuvqxyzABCDEFGHIJKLMNOPQRSTUVQXYZ0123456789-._~";
    assert_eq!(percent_encode(s), s);
}

#[test]
fn percent_encode_reserved_chars_encoded() {
    let s = "abcd/#$@%@(&%&!*@#)$%@!#dsfsdf0932510294";
    assert_eq!(
        percent_encode(s),
        "abcd%2f%23%24%40%25%40%28%26%25%26%21%2a%40%23%29%24%25%40%21%23dsfsdf0932510294"
    );
}

#[test]
fn percent_encode_multi_byte() {
    assert_eq!(percent_encode("À"), "%c3%80");
    assert_eq!(percent_encode("ア"), "%e3%82%a2");
    assert_eq!(percent_encode("💖"), "%f0%9f%92%96");
}

fn percent_decode(s: &str) -> Result<String> {
    let mut decoded = vec![];
    let mut parser = Parser::new(s);
    while let Some(c) = parser.parse_char().ok() {
        if c == '%' {
            let h1 = parser.parse_char()?;
            let h2 = parser.parse_char()?;
            decoded.push(u8::from_str_radix(&format!("{}{}", h1, h2), 16)?);
        } else {
            decoded.push(c as u8);
        }
    }
    Ok(str::from_utf8(&decoded)?.into())
}

#[test]
fn percent_decode_single_byte() {
    assert_eq!(percent_decode("%2f").unwrap(), "/");
    assert_eq!(percent_decode("%2F").unwrap(), "/");
    assert!(percent_decode("%a1").is_err());
}

#[test]
fn percent_decode_multi_byte() {
    assert_eq!(percent_decode("%c3%80").unwrap(), "À");
    assert_eq!(percent_decode("%e3%82%A2").unwrap(), "ア");
    assert_eq!(percent_decode("%f0%9f%92%96").unwrap(), "💖");
}

#[test]
fn percent_encode_round_trip() {
    let s = "abcd/#$@%@(&%&!*@#)$%@!#dsfsdf0932510294";
    assert_eq!(percent_decode(&percent_encode(s)).unwrap(), s);
}

#[test]
fn percent_decode_error() {
    assert!(percent_decode("%FG").is_err());
}

#[derive(PartialEq, Debug, Clone)]
pub struct Path {
    components: Vec<String>,
}

#[cfg(test)]
impl Path {
    fn new(components: &[&str]) -> Self {
        Self {
            components: components.iter().map(|&c| c.into()).collect(),
        }
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "/{}",
            self.components
                .iter()
                .map(|c| percent_encode(c.as_ref()))
                .collect::<Vec<_>>()
                .join("/")
        )
    }
}

impl str::FromStr for Path {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(Path {
            components: s
                .split("/")
                .filter(|s| s.len() > 0)
                .map(percent_decode)
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct Url {
    pub scheme: Scheme,
    pub authority: String,
    pub port: Option<u16>,
    pub path: Path,
    pub query: Option<String>,
    pub fragment: Option<String>,
    pub user_information: Option<String>,
}

impl Url {
    #[cfg(test)]
    fn new<S1: Into<String>, S2: Into<String>, S3: Into<String>, S4: Into<String>>(
        scheme: Scheme,
        authority: S1,
        port: Option<u16>,
        path: Path,
        query: Option<S2>,
        fragment: Option<S3>,
        user_information: Option<S4>,
    ) -> Self {
        Self {
            scheme,
            authority: authority.into(),
            port,
            path,
            query: query.map(Into::into),
            fragment: fragment.map(Into::into),
            user_information: user_information.map(Into::into),
        }
    }

    pub fn path(&self) -> String {
        format!(
            "{}{}{}",
            self.path,
            self.query
                .as_ref()
                .map(|d| format!("?{}", percent_encode(d)))
                .unwrap_or("".into()),
            self.fragment
                .as_ref()
                .map(|d| format!("#{}", percent_encode(d)))
                .unwrap_or("".into()),
        )
    }

    pub fn port(&self) -> Result<u16> {
        if let Some(p) = self.port {
            return Ok(p);
        }
        match &self.scheme {
            Scheme::Http => Ok(80),
            Scheme::Https => Ok(443),
            s => Err(Error::UrlError(format!("port for {} not known", s))),
        }
    }
}

impl TryFrom<&str> for Url {
    type Error = Error;
    fn try_from(value: &str) -> Result<Self> {
        value.parse()
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}://{}{}{}{}",
            self.scheme,
            self.user_information
                .as_ref()
                .map(|d| format!("{}@", d))
                .unwrap_or("".into()),
            self.authority,
            self.port
                .as_ref()
                .map(|d| format!(":{}", d))
                .unwrap_or("".into()),
            self.path()
        )
    }
}

impl str::FromStr for Url {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let mut parser = Parser::new(s);

        let scheme: Scheme = str::parse(parser.parse_until(":")?)?;
        parser.expect("://")?;

        let user_information = match parser.parse_until("@") {
            Ok(info) => {
                parser.expect("@").unwrap();
                Some(info.into())
            }
            _ => None,
        };

        let authority = parser
            .parse_until_any(&['/', '?', '#', ':'])
            .or_else(|_| parser.parse_remaining())?
            .into();

        let port = match parser.expect(":") {
            Ok(_) => Some(
                parser
                    .parse_until_any(&['/', '?', '#'])
                    .or_else(|_| parser.parse_remaining())?
                    .parse()?,
            ),
            _ => None,
        };

        parser.expect("/").ok();

        let path = parser
            .parse_until_any(&['?', '#'])
            .or_else(|_| parser.parse_remaining())
            .unwrap_or("")
            .parse()?;

        let query = match parser.expect("?") {
            Ok(_) => Some(percent_decode(
                parser
                    .parse_until("#")
                    .or_else(|_| parser.parse_remaining())
                    .unwrap_or(""),
            )?),
            Err(_) => None,
        };

        let fragment = match parser.expect("#") {
            Ok(_) => Some(percent_decode(parser.parse_remaining().unwrap_or(""))?),
            Err(_) => None,
        };

        Ok(Self {
            scheme,
            authority,
            port,
            path,
            query,
            fragment,
            user_information,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip_test(s: &str) {
        let url: Url = str::parse(s).unwrap();
        assert_eq!(&format!("{}", url), s);
    }

    #[test]
    fn parse_round_trip() {
        round_trip_test("http://google.com/");
        round_trip_test("https://google.com/");
        round_trip_test("http://google.com/something.html");
        round_trip_test("ftp://google.com/something.html");
        round_trip_test("ftp://google.com/something.html?foo#bar");
        round_trip_test("ftp://google.com/something.html#bar%3ffoo");
        round_trip_test("ftp://www.google.com/pie");
        round_trip_test("ftp://user:pass@www.google.com/pie");
        round_trip_test("ftp://user:pass@www.google.com:9090/pie");
        round_trip_test("http://www.google.com/%2fderp%2fface");
        round_trip_test("http://www.google.com/?%2fderp%2fface");
        round_trip_test("http://www.google.com/#%2fderp%2fface");
        round_trip_test("http://www.google.com/?#");
    }

    fn parse_test(
        input: &str,
        scheme: Scheme,
        authority: &str,
        port: Option<u16>,
        path: &[&str],
        query: Option<&str>,
        fragment: Option<&str>,
        user_information: Option<&str>,
    ) {
        let actual_url: Url = str::parse(input).unwrap();
        let expected_url = Url::new(
            scheme,
            authority,
            port,
            Path::new(path),
            query,
            fragment,
            user_information,
        );
        assert_eq!(actual_url, expected_url);
    }

    #[test]
    fn parse_simple() {
        parse_test(
            "http://google.com",
            Scheme::Http,
            "google.com",
            None,
            &[],
            None,
            None,
            None,
        );
        parse_test(
            "https://google.com/",
            Scheme::Https,
            "google.com",
            None,
            &[],
            None,
            None,
            None,
        );
        parse_test(
            "ftp://www.google.com/a/b/c",
            Scheme::Other("ftp".into()),
            "www.google.com",
            None,
            &["a", "b", "c"],
            None,
            None,
            None,
        );
    }

    #[test]
    fn parse_path_encoding() {
        parse_test(
            "http://google.com/%2ffoo%2fbar",
            Scheme::Http,
            "google.com",
            None,
            &["/foo/bar"],
            None,
            None,
            None,
        );
    }

    #[test]
    fn parse_query() {
        parse_test(
            "http://google.com?foobar",
            Scheme::Http,
            "google.com",
            None,
            &[],
            Some("foobar"),
            None,
            None,
        );
    }

    #[test]
    fn parse_fragment() {
        parse_test(
            "http://google.com#foobar",
            Scheme::Http,
            "google.com",
            None,
            &[],
            None,
            Some("foobar"),
            None,
        );
    }

    #[test]
    fn parse_query_and_fragment() {
        parse_test(
            "http://google.com?foo#bar",
            Scheme::Http,
            "google.com",
            None,
            &[],
            Some("foo"),
            Some("bar"),
            None,
        );
    }

    #[test]
    fn parse_fragment_and_query() {
        parse_test(
            "http://google.com#bar?foo",
            Scheme::Http,
            "google.com",
            None,
            &[],
            None,
            Some("bar?foo"),
            None,
        );
    }

    #[test]
    fn parse_credentials() {
        parse_test(
            "https://user:pass@google.com/something",
            Scheme::Https,
            "google.com",
            None,
            &["something"],
            None,
            None,
            Some("user:pass"),
        );
    }

    #[test]
    fn parse_port() {
        parse_test(
            "http://google.com:8080#foobar",
            Scheme::Http,
            "google.com",
            Some(8080),
            &[],
            None,
            Some("foobar"),
            None,
        );
    }

    #[test]
    fn scheme_to_port() -> Result<()> {
        let url: Url = "http://google.com".parse()?;
        assert_eq!(url.port()?, 80);

        let url: Url = "https://google.com".parse()?;
        assert_eq!(url.port()?, 443);

        let url: Url = "http://google.com:9090".parse()?;
        assert_eq!(url.port()?, 9090);

        let url: Url = "file://google.com".parse()?;
        assert!(url.port().is_err());

        let url: Url = "derp://google.com".parse()?;
        assert!(url.port().is_err());

        Ok(())
    }
}
