use crate::error::{Error, Result};
#[cfg(not(feature = "std"))]
use alloc::format;
#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString};
use core::convert::TryFrom;
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

#[derive(PartialEq, Debug, Clone)]
pub struct HttpUrl {
    url: Url,
    scheme: Scheme,
    host: String,
}

impl HttpUrl {
    pub fn port(&self) -> Result<u16> {
        match self.url.port_or_known_default() {
            Some(v) => Ok(v),
            None => Err(Error::UrlError(format!(
                "unsupported URL scheme {}: no port and no known default",
                self.url.scheme()
            ))),
        }
    }
    pub fn scheme(&self) -> Scheme {
        self.scheme.clone()
    }
    pub fn host(&self) -> &str {
        &self.host
    }
    // give use an option to use the origin url struct and save us from porting too many codes
    pub fn url(&self) -> &Url {
        &self.url
    }
}

impl TryFrom<Url> for HttpUrl {
    type Error = Error;

    fn try_from(url: Url) -> Result<Self> {
        use core::str::FromStr;

        let scheme = Scheme::from_str(url.scheme())?;
        if scheme.ne(&Scheme::Http) && scheme.ne(&Scheme::Https) {
            return Err(Error::UrlError(format!(
                "unsupported URL scheme {}",
                url.scheme()
            )));
        };
        let Some(host) = url.host_str() else {
            // people can use url.set_host(Option::None) to make host invalid, and currently we
            // don't support that
            return Err(Error::UrlError(format!(
                "unsupported URL scheme {}: no host",
                url.scheme()
            )));
        };
        Ok(Self {
            scheme,
            host: String::from(host),
            url,
        })
    }
}

impl str::FromStr for HttpUrl {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let url = Url::parse(s).map_err(|err| Error::UrlError(err.to_string()))?;
        HttpUrl::try_from(url)
    }
}

impl TryFrom<&str> for HttpUrl {
    type Error = Error;

    fn try_from(value: &str) -> Result<Self> {
        value.parse()
    }
}

impl fmt::Display for HttpUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.url.fmt(f)
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

    fn parse_http_url_test(url: &str, scheme: Scheme, host: &str, port: u16) {
        let http_url: HttpUrl = url.parse().unwrap();
        assert_eq!(http_url.scheme(), scheme);
        assert_eq!(http_url.host(), host);
        assert_eq!(http_url.port().unwrap(), port);
    }

    #[test]
    fn parse_http_url() {
        // test parse http url from str, mainly watch the scheme, host and port
        parse_http_url_test("http://a.com/b/c/d", Scheme::Http, "a.com", 80);
        parse_http_url_test("https://a.com/b/c/d", Scheme::Https, "a.com", 443);
        parse_http_url_test("http://a.com:9000/b/c/d", Scheme::Http, "a.com", 9000);
        parse_http_url_test("https://a.com:9000/b/c/d", Scheme::Https, "a.com", 9000);
    }
}
