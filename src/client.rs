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
//!
//!     // Stream contents of file to remote server
//!     let file = File::open("src/client.rs")?;
//!     http_io::client::put("http://www.google.com", file)?;
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
use crate::protocol::{HttpBody, HttpStatus};
use crate::protocol::{HttpMethod, HttpRequest, OutgoingBody};
use crate::url::Url;
#[cfg(not(feature = "std"))]
use alloc::string::ToString;
use core::convert::TryInto;
use core::fmt::Display;
use core::hash::Hash;
use hashbrown::HashMap;

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
        request.add_header("Host", &url.authority);
        request.add_header("User-Agent", "http_io");
        request.add_header("Accept", "*/*");
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
                .ok_or(err())?,
        )
    }
}

/// An HTTP client that keeps connections open.
pub struct HttpClient<S: StreamConnector> {
    streams: HashMap<S::stream_addr, S::stream>,
}

impl<S: StreamConnector> HttpClient<S> {
    /// Create an `HTTPClient`
    pub fn new() -> Self {
        HttpClient {
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

/// Execute a GET request.
///
/// *This function is available if http_io is built with the `"std"` feature.*
#[cfg(feature = "std")]
pub fn get<U: TryInto<Url>>(url: U) -> Result<HttpBody<std::net::TcpStream>>
where
    <U as TryInto<Url>>::Error: Display,
{
    let url = url
        .try_into()
        .map_err(|e| Error::ParseError(e.to_string()))?;
    let s = std::net::TcpStream::connect((url.authority.as_ref(), url.port()?))?;
    let response = HttpRequestBuilder::get(url)?.send(s)?.finish()?;

    if response.status != HttpStatus::OK {
        return Err(Error::UnexpectedStatus(response.status));
    }

    Ok(response.body)
}

/// Execute a PUT request.
///
/// *This function is available if http_io is built with the `"std"` feature.*
#[cfg(feature = "std")]
pub fn put<U: TryInto<Url>, R: io::Read>(
    url: U,
    mut body: R,
) -> Result<HttpBody<std::net::TcpStream>>
where
    <U as TryInto<Url>>::Error: Display,
{
    let url = url
        .try_into()
        .map_err(|e| Error::ParseError(e.to_string()))?;
    let s = std::net::TcpStream::connect((url.authority.as_ref(), url.port()?))?;
    let mut request = HttpRequestBuilder::get(url)?.send(s)?;

    io::copy(&mut body, &mut request)?;

    let response = request.finish()?;

    if response.status != HttpStatus::OK {
        return Err(Error::UnexpectedStatus(response.status));
    }

    Ok(response.body)
}
