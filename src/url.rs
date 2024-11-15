use crate::error::{Error, Result};
#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::fmt;
use core::str;
pub use url::Url;

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


#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::str::FromStr;

    fn round_trip_test(s: &str) {
        let url: Url = str::parse(s).unwrap();
        assert_eq!(&std::format!("{}", url), s);
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
        path: &str,
        query: Option<&str>,
        fragment: Option<&str>,
    ) {
        let url = Url::parse(input).unwrap();
        assert_eq!(Scheme::from_str(url.scheme()).unwrap(), scheme);
        assert_eq!(url.authority(), authority);
        assert_eq!(url.port(), port);
        assert_eq!(url.path(), path);
        assert_eq!(url.query(), query);
        assert_eq!(url.fragment(), fragment);
    }

    #[test]
    fn parse_simple() {
        parse_test(
            "http://google.com",
            Scheme::Http,
            "google.com",
            None,
            "/",
            None,
            None,
        );
        parse_test(
            "https://google.com/",
            Scheme::Https,
            "google.com",
            None,
            "/",
            None,
            None,
        );
        parse_test(
            "https://google.com/a/b/c/",
            Scheme::Https,
            "google.com",
            None,
            "/a/b/c/",
            None,
            None,
        );
        parse_test(
            "ftp://www.google.com/a/b/c",
            Scheme::Other("ftp".into()),
            "www.google.com",
            None,
            "/a/b/c",
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
            "/",
            Some("foobar"),
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
            "/",
            None,
            Some("foobar"),
        );
    }

    #[test]
    fn parse_query_and_fragment() {
        parse_test(
            "http://google.com?foo#bar",
            Scheme::Http,
            "google.com",
            None,
            "/",
            Some("foo"),
            Some("bar"),
        );
    }

    #[test]
    fn parse_fragment_and_query() {
        parse_test(
            "http://google.com#bar?foo",
            Scheme::Http,
            "google.com",
            None,
            "/",
            None,
            Some("bar?foo"),
        );
    }

    #[test]
    fn parse_credentials() {
        parse_test(
            "https://user:pass@google.com/something",
            Scheme::Https,
            "user:pass@google.com",
            None,
            "/something",
            None,
            None,
        );
    }

    #[test]
    fn parse_port() {
        parse_test(
            "http://google.com:8080#foobar",
            Scheme::Http,
            "google.com:8080",
            Some(8080),
            "/",
            None,
            Some("foobar"),
        );
    }

    #[test]
    fn scheme_to_port() -> Result<()> {
        let url = Url::parse("http://google.com").unwrap();
        assert_eq!(url.port_or_known_default(), Option::Some(80));

        let url = Url::parse("https://google.com").unwrap();
        assert_eq!(url.port_or_known_default(), Option::Some(443));

        let url = Url::parse("http://google.com:9090").unwrap();
        assert_eq!(url.port_or_known_default(), Option::Some(9090));

        let url = Url::parse("file://google.com").unwrap();
        assert_eq!(url.port_or_known_default(), Option::None);

        let url = Url::parse("derp://google.com").unwrap();
        assert_eq!(url.port_or_known_default(), Option::None);

        Ok(())
    }
}
