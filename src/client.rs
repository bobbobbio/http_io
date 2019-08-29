//! Code for making HTTP requests.
//!
//! # Examples
//!
//! # Making simple requests
//! ```rust
//! use http_io::error::Result;
//! use std::fs::File;
//! use std::io;
//!
//! fn main() -> Result<()> {
//!     // Stream contents of url to stdout
//!     let mut body = http_io::client::get("http://www.google.com")?;
//!     io::copy(&mut body, &mut std::io::stdout())?;
//!     Ok(())
//! }
//! ```
//! # Using the `HttpRequestBuilder` for more control
//!
//! ```rust
//! use http_io::client::HttpRequestBuilder;
//! use http_io::error::Result;
//! use http_io::url::Url;
//! use std::io;
//! use std::net::TcpStream;
//!
//! fn main() -> Result<()> {
//!     let url: Url = "http://www.google.com".parse()?;
//!     let s = TcpStream::connect((url.authority.as_ref(), url.port()?))?;
//!     let mut response = HttpRequestBuilder::get(url)?.send(s)?.finish()?;
//!     println!("{:#?}", response.headers);
//!     io::copy(&mut response.body, &mut io::stdout())?;
//!     Ok(())
//! }
//! ```
//! # Using `HttpClient` to keep connections open
//! ```rust
//! use http_io::client::HttpClient;
//! use http_io::error::Result;
//! use http_io::url::Url;
//! use std::io;
//!
//! fn main() -> Result<()> {
//!     let url: Url = "http://www.google.com".parse()?;
//!     let mut client = HttpClient::<std::net::TcpStream>::new();
//!     for path in &["/", "/favicon.ico", "/robots.txt"] {
//!         let mut url = url.clone();
//!         url.path = path.parse()?;
//!         io::copy(&mut client.get(url)?.finish()?.body, &mut io::stdout())?;
//!     }
//!     Ok(())
//! }
//!```

use crate::error::{Error, Result};
use crate::io;
#[cfg(feature = "std")]
use crate::protocol::HttpStatus;
use crate::protocol::{HttpMethod, HttpRequest, OutgoingBody};
#[cfg(feature = "std")]
use crate::url::Scheme;
use crate::url::Url;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;
use core::convert::TryInto;
use core::fmt::Display;
use core::hash::Hash;
use hashbrown::HashMap;
#[cfg(feature = "openssl")]
use openssl::ssl::{SslConnector, SslMethod};

/// A struct for building up an HTTP request.
pub struct HttpRequestBuilder {
    request: HttpRequest<io::Empty>,
}

impl HttpRequestBuilder {
    /// Create a `HttpRequestBuilder` to build a GET request
    pub fn get<U: TryInto<Url>>(url: U) -> Result<Self>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        HttpRequestBuilder::new(url, HttpMethod::Get)
    }

    /// Create a `HttpRequestBuilder` to build a HEAD request
    pub fn head<U: TryInto<Url>>(url: U) -> Result<Self>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        HttpRequestBuilder::new(url, HttpMethod::Head)
    }

    /// Create a `HttpRequestBuilder` to build a PUT request
    pub fn put<U: TryInto<Url>>(url: U) -> Result<Self>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        HttpRequestBuilder::new(url, HttpMethod::Put)
    }

    /// Create a `HttpRequestBuilder`. May fail if the given url does not parse.
    pub fn new<U: TryInto<Url>>(url: U, method: HttpMethod) -> Result<Self>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        let url = url
            .try_into()
            .map_err(|e| Error::ParseError(e.to_string()))?;
        let mut request = HttpRequest::new(method, url.path());
        request.add_header("Host", url.authority.clone());
        request.add_header("User-Agent", "http_io");
        request.add_header("Accept", "*/*");
        request.add_header("Transfer-Encoding", "chunked");
        Ok(HttpRequestBuilder { request })
    }

    /// Send the built request on the given socket
    pub fn send<S: io::Read + io::Write>(self, socket: S) -> Result<OutgoingBody<S>> {
        self.request.serialize(io::BufWriter::new(socket))
    }

    /// Add a header to the request
    pub fn add_header<S1: AsRef<str>, S2: AsRef<str>>(mut self, key: S1, value: S2) -> Self {
        self.request.add_header(key.as_ref(), value.as_ref());
        self
    }
}

/// Represents the ability to connect an abstract stream to some destination address.
pub trait StreamConnector {
    type stream: io::Read + io::Write;
    type stream_addr: Hash + Eq + Clone;
    fn connect(a: Self::stream_addr) -> Result<Self::stream>;
    fn to_stream_addr(url: Url) -> Result<Self::stream_addr>;
}

#[cfg(feature = "std")]
impl StreamConnector for std::net::TcpStream {
    type stream = std::net::TcpStream;
    type stream_addr = std::net::SocketAddr;

    fn connect(a: Self::stream_addr) -> Result<Self::stream> {
        Ok(std::net::TcpStream::connect(a)?)
    }

    fn to_stream_addr(url: Url) -> Result<Self::stream_addr> {
        let err = || {
            std::io::Error::new(
                std::io::ErrorKind::AddrNotAvailable,
                format!("Failed to lookup {}", &url.authority),
            )
        };
        Ok(
            std::net::ToSocketAddrs::to_socket_addrs(&(url.authority.as_ref(), url.port()?))
                .map_err(|_| err())?
                .next()
                .ok_or_else(err)?,
        )
    }
}

/// An HTTP client that keeps connections open.
pub struct HttpClient<S: StreamConnector> {
    streams: HashMap<S::stream_addr, S::stream>,
}

impl<S: StreamConnector> HttpClient<S> {
    /// Create an `HTTPClient`
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
        }
    }

    fn get_socket(&mut self, url: Url) -> Result<&mut S::stream> {
        let stream_addr = S::to_stream_addr(url)?;
        if !self.streams.contains_key(&stream_addr) {
            let stream = S::connect(stream_addr.clone())?;
            self.streams.insert(stream_addr.clone(), stream);
        }
        Ok(self.streams.get_mut(&stream_addr).unwrap())
    }

    /// Execute a GET request. The request isn't completed until `OutgoingBody::finish` is called.
    pub fn get<U: TryInto<Url>>(&mut self, url: U) -> Result<OutgoingBody<&mut S::stream>>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        let url = url
            .try_into()
            .map_err(|e| Error::ParseError(e.to_string()))?;
        Ok(HttpRequestBuilder::get(url.clone())?.send(self.get_socket(url)?)?)
    }

    /// Execute a PUT request. The request isn't completed until `OutgoingBody::finish` is called.
    pub fn put<U: TryInto<Url>>(&mut self, url: U) -> Result<OutgoingBody<&mut S::stream>>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        let url = url
            .try_into()
            .map_err(|e| Error::ParseError(e.to_string()))?;
        Ok(HttpRequestBuilder::put(url.clone())?.send(self.get_socket(url)?)?)
    }
}

#[cfg(feature = "std")]
fn send_request<R: io::Read>(
    builder: HttpRequestBuilder,
    url: Url,
    mut body: R,
) -> Result<Box<dyn io::Read>> {
    let stream = std::net::TcpStream::connect((url.authority.as_ref(), url.port()?))?;
    let (status, body) = match &url.scheme {
        #[cfg(feature = "openssl")]
        Scheme::Https => {
            // XXX I need a front-door way to support self-signed certificates.
            #[allow(unused_mut)]
            let mut connector = SslConnector::builder(SslMethod::tls())?;
            #[cfg(test)]
            connector.set_verify(openssl::ssl::SslVerifyMode::NONE);
            let connector = connector.build();
            let stream = connector.connect(&url.authority, stream)?;
            let mut request = builder.send(stream)?;
            io::copy(&mut body, &mut request)?;
            let response = request.finish()?;
            (
                response.status,
                Box::new(response.body) as Box<dyn io::Read>,
            )
        }
        Scheme::Http => {
            let mut request = builder.send(stream)?;
            io::copy(&mut body, &mut request)?;
            let response = request.finish()?;
            (
                response.status,
                Box::new(response.body) as Box<dyn io::Read>,
            )
        }
        s => {
            return Err(Error::UnexpectedScheme(s.to_string()));
        }
    };

    if status != HttpStatus::OK {
        return Err(Error::UnexpectedStatus(status));
    }

    Ok(body)
}

#[cfg(test)]
use crate::server::{
    test_server, test_ssl_server, ExpectedRequest, HttpRequestHandler, HttpServer, Listen,
};

/// Execute a GET request.
///
/// *This function is available if http_io is built with the `"std"` feature.*
#[cfg(feature = "std")]
pub fn get<U: TryInto<Url>>(url: U) -> Result<Box<dyn io::Read>>
where
    <U as TryInto<Url>>::Error: Display,
{
    let url = url
        .try_into()
        .map_err(|e| Error::ParseError(e.to_string()))?;
    let builder = HttpRequestBuilder::get(url.clone())?;
    Ok(send_request(builder, url, io::empty())?)
}

#[cfg(test)]
fn get_test<
    L: Listen + Send + 'static,
    T: HttpRequestHandler<L::stream> + Send + 'static,
    F: Fn(Vec<ExpectedRequest>) -> Result<(u16, HttpServer<L, T>)>,
>(
    scheme: Scheme,
    server_factor: F,
) -> Result<()> {
    let (port, mut server) = server_factor(vec![ExpectedRequest {
        expected_method: HttpMethod::Get,
        expected_uri: "/".into(),
        expected_body: "".into(),
        response_status: HttpStatus::OK,
        response_body: "hello from server".into(),
    }])?;
    let handle = std::thread::spawn(move || server.serve_one());
    let mut body = get(format!("{}://localhost:{}/", scheme, port).as_ref())?;
    handle.join().unwrap()?;

    let mut body_str = String::new();
    body.read_to_string(&mut body_str)?;
    assert_eq!(body_str, "hello from server");
    Ok(())
}

#[test]
fn get_request() -> Result<()> {
    get_test(Scheme::Http, test_server)
}

#[test]
fn get_request_ssl() -> Result<()> {
    get_test(Scheme::Https, test_ssl_server)
}

/// Execute a PUT request.
///
/// *This function is available if http_io is built with the `"std"` feature.*
#[cfg(feature = "std")]
pub fn put<U: TryInto<Url>, R: io::Read>(url: U, body: R) -> Result<Box<dyn io::Read>>
where
    <U as TryInto<Url>>::Error: Display,
{
    let url = url
        .try_into()
        .map_err(|e| Error::ParseError(e.to_string()))?;
    let builder = HttpRequestBuilder::put(url.clone())?;
    Ok(send_request(builder, url, body)?)
}

#[cfg(test)]
fn put_test<
    L: Listen + Send + 'static,
    T: HttpRequestHandler<L::stream> + Send + 'static,
    F: Fn(Vec<ExpectedRequest>) -> Result<(u16, HttpServer<L, T>)>,
>(
    scheme: Scheme,
    server_factor: F,
) -> Result<()> {
    let (port, mut server) = server_factor(vec![ExpectedRequest {
        expected_method: HttpMethod::Put,
        expected_uri: "/".into(),
        expected_body: "hello from client".into(),
        response_status: HttpStatus::OK,
        response_body: "hello from server".into(),
    }])?;
    let handle = std::thread::spawn(move || server.serve_one());

    let mut incoming_body = put(
        format!("{}://localhost:{}/", scheme, port).as_ref(),
        "hello from client".as_bytes(),
    )?;

    handle.join().unwrap()?;

    let mut body_str = String::new();
    incoming_body.read_to_string(&mut body_str)?;
    assert_eq!(body_str, "hello from server");
    Ok(())
}

#[test]
fn put_request() -> Result<()> {
    put_test(Scheme::Http, test_server)
}

#[test]
fn put_request_ssl() -> Result<()> {
    put_test(Scheme::Https, test_ssl_server)
}
