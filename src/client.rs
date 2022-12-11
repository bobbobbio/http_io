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
//!     let mut body = http_io::client::get("http://abort.cc")?;
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
use crate::protocol::{HttpBody, HttpStatus};
use crate::protocol::{HttpMethod, HttpRequest, OutgoingRequest};
#[cfg(feature = "std")]
use crate::url::Scheme;
use crate::url::Url;
#[cfg(not(feature = "std"))]
use alloc::string::{String, ToString as _};
use core::convert::TryInto;
use core::fmt::Display;
use core::hash::Hash;
use hashbrown::HashMap;

/// A struct for building up an HTTP request.
pub struct HttpRequestBuilder {
    request: HttpRequest<io::Empty>,
}

impl HttpRequestBuilder {
    /// Create a `HttpRequestBuilder` to build a DELETE request
    pub fn delete<U: TryInto<Url>>(url: U) -> Result<Self>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        HttpRequestBuilder::new(url, HttpMethod::Delete)
    }

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

    /// Create a `HttpRequestBuilder` to build an OPTIONS request
    pub fn options<U: TryInto<Url>>(url: U) -> Result<Self>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        HttpRequestBuilder::new(url, HttpMethod::Options)
    }

    /// Create a `HttpRequestBuilder` to build a POST request
    pub fn post<U: TryInto<Url>>(url: U) -> Result<Self>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        HttpRequestBuilder::new(url, HttpMethod::Post)
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
        if method.has_body() {
            request.add_header("Transfer-Encoding", "chunked");
        }
        Ok(HttpRequestBuilder { request })
    }

    /// Send the built request on the given socket
    pub fn send<S: io::Read + io::Write>(self, socket: S) -> Result<OutgoingRequest<S>> {
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
    type Stream: io::Read + io::Write;
    type StreamAddr: Hash + Eq + Clone;
    fn connect(a: Self::StreamAddr) -> Result<Self::Stream>;
    fn to_stream_addr(url: Url) -> Result<Self::StreamAddr>;
}

pub enum StreamEither<A, B> {
    A(A),
    B(B),
}

impl<A: io::Read, B: io::Read> io::Read for StreamEither<A, B> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::A(a) => a.read(buf),
            Self::B(b) => b.read(buf),
        }
    }
}

impl<A: io::Write, B: io::Write> io::Write for StreamEither<A, B> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::A(a) => a.write(buf),
            Self::B(b) => b.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::A(a) => a.flush(),
            Self::B(b) => b.flush(),
        }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct StreamId<Addr> {
    addr: Addr,
    host: String,
    secure: bool,
}

#[cfg(all(feature = "std", feature = "ssl"))]
pub type StdTransport =
    StreamEither<std::net::TcpStream, crate::ssl::SslClientStream<std::net::TcpStream>>;

#[cfg(all(feature = "std", not(feature = "ssl")))]
pub type StdTransport = std::net::TcpStream;

#[cfg(feature = "std")]
impl StreamConnector for std::net::TcpStream {
    type Stream = StdTransport;
    type StreamAddr = StreamId<std::net::SocketAddr>;

    #[cfg(not(feature = "ssl"))]
    fn connect(id: Self::StreamAddr) -> Result<Self::Stream> {
        Ok(std::net::TcpStream::connect(id.addr)?)
    }

    #[cfg(feature = "ssl")]
    fn connect(id: Self::StreamAddr) -> Result<Self::Stream> {
        let s = std::net::TcpStream::connect(id.addr)?;
        if id.secure {
            Ok(StreamEither::B(crate::ssl::SslClientStream::new(
                &id.host, s,
            )?))
        } else {
            Ok(StreamEither::A(s))
        }
    }

    fn to_stream_addr(url: Url) -> Result<Self::StreamAddr> {
        let err = || {
            std::io::Error::new(
                std::io::ErrorKind::AddrNotAvailable,
                format!("Failed to lookup {}", &url.authority),
            )
        };
        Ok(StreamId {
            addr: std::net::ToSocketAddrs::to_socket_addrs(&(url.authority.as_ref(), url.port()?))
                .map_err(|_| err())?
                .next()
                .ok_or_else(err)?,
            host: url.authority,
            secure: url.scheme == Scheme::Https,
        })
    }
}

/// An HTTP client that keeps connections open.
pub struct HttpClient<S: StreamConnector> {
    streams: HashMap<S::StreamAddr, S::Stream>,
}

impl<S: StreamConnector> HttpClient<S> {
    /// Create an `HTTPClient`
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
        }
    }

    fn get_stream(&mut self, url: Url) -> Result<&mut S::Stream> {
        let stream_addr = S::to_stream_addr(url)?;
        if !self.streams.contains_key(&stream_addr) {
            let stream = S::connect(stream_addr.clone())?;
            self.streams.insert(stream_addr.clone(), stream);
        }
        Ok(self.streams.get_mut(&stream_addr).unwrap())
    }

    /// Execute a GET request. The request isn't completed until `OutgoingRequest::finish` is
    /// called.
    pub fn get<U: TryInto<Url>>(&mut self, url: U) -> Result<OutgoingRequest<&mut S::Stream>>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        let url = url
            .try_into()
            .map_err(|e| Error::ParseError(e.to_string()))?;
        Ok(HttpRequestBuilder::get(url.clone())?.send(self.get_stream(url)?)?)
    }

    /// Execute a PUT request. The request isn't completed until `OutgoingRequest::finish` is
    /// called.
    pub fn put<U: TryInto<Url>>(&mut self, url: U) -> Result<OutgoingRequest<&mut S::Stream>>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        let url = url
            .try_into()
            .map_err(|e| Error::ParseError(e.to_string()))?;
        Ok(HttpRequestBuilder::put(url.clone())?.send(self.get_stream(url)?)?)
    }
}

#[cfg(feature = "std")]
fn send_request<R: io::Read>(
    builder: HttpRequestBuilder,
    url: Url,
    mut body: R,
) -> Result<HttpBody<StdTransport>> {
    use std::net::TcpStream;

    let stream = <TcpStream as StreamConnector>::connect(TcpStream::to_stream_addr(url)?)?;
    let mut request = builder.send(stream)?;
    io::copy(&mut body, &mut request)?;
    let response = request.finish()?;

    if response.status != HttpStatus::OK {
        return Err(Error::UnexpectedStatus(response.status));
    }

    Ok(response.body)
}

#[cfg(test)]
use crate::server::{
    test_server, test_ssl_server, ExpectedRequest, HttpRequestHandler, HttpServer, Listen,
};

#[cfg(test)]
use crate::http_headers;

/// Execute a GET request.
///
/// *This function is available if http_io is built with the `"std"` feature.*
#[cfg(feature = "std")]
pub fn get<U: TryInto<Url>>(url: U) -> Result<HttpBody<StdTransport>>
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
    T: HttpRequestHandler<L::Stream> + Send + 'static,
    B: io::Read,
>(
    scheme: Scheme,
    server_factory: impl Fn(Vec<ExpectedRequest>) -> Result<(u16, HttpServer<L, T>)>,
    requester: impl FnOnce(&str) -> Result<HttpBody<B>>,
) -> Result<()> {
    use std::io::Read as _;

    let (port, mut server) = server_factory(vec![ExpectedRequest {
        expected_method: HttpMethod::Get,
        expected_uri: "/".into(),
        expected_body: "".into(),
        response_status: HttpStatus::OK,
        response_body: "hello from server".into(),
        response_headers: Default::default(),
    }])?;
    let handle = std::thread::spawn(move || server.serve_one());
    let mut body = requester(format!("{}://localhost:{}/", scheme, port).as_ref())?;
    handle.join().unwrap()?;

    let mut body_str = String::new();
    body.read_to_string(&mut body_str)?;
    assert_eq!(body_str, "hello from server");
    Ok(())
}

#[test]
fn get_request() {
    get_test(Scheme::Http, test_server, |a| get(a)).unwrap();
}

#[test]
fn http_client_get_request() {
    let mut client = HttpClient::<std::net::TcpStream>::new();
    get_test(Scheme::Http, test_server, |a| {
        Ok(client.get(a)?.finish()?.body)
    })
    .unwrap();
}

#[test]
fn get_request_ssl() {
    get_test(
        Scheme::Https,
        |s| test_ssl_server("test_key.pem", "test_cert.pem", s),
        |a| get(a),
    )
    .unwrap();
}

#[test]
fn http_client_get_request_ssl() {
    let mut client = HttpClient::<std::net::TcpStream>::new();
    get_test(
        Scheme::Https,
        |s| test_ssl_server("test_key.pem", "test_cert.pem", s),
        |a| Ok(client.get(a)?.finish()?.body),
    )
    .unwrap();
}

/// Execute a PUT request.
///
/// *This function is available if http_io is built with the `"std"` feature.*
#[cfg(feature = "std")]
pub fn put<U: TryInto<Url>, R: io::Read>(url: U, body: R) -> Result<HttpBody<StdTransport>>
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
    T: HttpRequestHandler<L::Stream> + Send + 'static,
    B: io::Read,
>(
    scheme: Scheme,
    server_factory: impl Fn(Vec<ExpectedRequest>) -> Result<(u16, HttpServer<L, T>)>,
    requester: impl FnOnce(&str, &[u8]) -> Result<HttpBody<B>>,
) -> Result<()> {
    use std::io::Read as _;

    let (port, mut server) = server_factory(vec![ExpectedRequest {
        expected_method: HttpMethod::Put,
        expected_uri: "/".into(),
        expected_body: "hello from client".into(),
        response_status: HttpStatus::OK,
        response_body: "hello from server".into(),
        response_headers: Default::default(),
    }])?;
    let handle = std::thread::spawn(move || server.serve_one());

    let mut incoming_body = requester(
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
fn put_request() {
    put_test(Scheme::Http, test_server, |a, b| put(a, b)).unwrap();
}

#[cfg(test)]
fn client_put<'a>(
    client: &'a mut HttpClient<std::net::TcpStream>,
    url: &str,
    mut body: &[u8],
) -> Result<HttpBody<&'a mut StdTransport>> {
    let mut out = client.put(url)?;
    io::copy(&mut body, &mut out)?;
    Ok(out.finish()?.body)
}

#[test]
fn http_client_put_request() {
    let mut client = HttpClient::<std::net::TcpStream>::new();
    put_test(Scheme::Http, test_server, |a, b| {
        client_put(&mut client, a, b)
    })
    .unwrap();
}

#[test]
fn put_request_ssl() {
    put_test(
        Scheme::Https,
        |s| test_ssl_server("test_key.pem", "test_cert.pem", s),
        |a, b| put(a, b),
    )
    .unwrap();
}

#[test]
fn http_client_put_request_ssl() {
    let mut client = HttpClient::<std::net::TcpStream>::new();
    put_test(
        Scheme::Https,
        |s| test_ssl_server("test_key.pem", "test_cert.pem", s),
        |a, b| client_put(&mut client, a, b),
    )
    .unwrap();
}

#[test]
fn get_ssl_success() {
    use std::io::Read as _;

    for u in ["https://remi.party/", "https://www.google.com"] {
        let mut client = HttpClient::<std::net::TcpStream>::new();
        let mut body = client.get(u).unwrap().finish().unwrap().body;
        let mut body_bytes = Vec::new();
        body.read_to_end(&mut body_bytes).unwrap();
    }
}

#[test]
fn get_ssl_failure() {
    let mut client = HttpClient::<std::net::TcpStream>::new();
    let err = client.get("https://abort.cc/").err().unwrap();
    assert!(matches!(err, Error::SslError(_)));
}

#[test]
fn get_ssl_bad_certificate_name() {
    // These certificates have a hostname different from localhost, so the hostname verification
    // should fail.
    let err = get_test(
        Scheme::Https,
        |s| test_ssl_server("test_bad_key.pem", "test_bad_cert.pem", s),
        |a| get(a),
    )
    .unwrap_err();
    assert!(matches!(err, Error::SslError(_)));
}

#[ignore]
#[test]
fn redirect() {
    use std::io::Read as _;

    let (port, mut server) = test_server(vec![
        ExpectedRequest {
            expected_method: HttpMethod::Get,
            expected_uri: "/".into(),
            expected_body: "".into(),
            response_status: HttpStatus::MovedPermanently,
            response_body: "".into(),
            response_headers: http_headers! {
                "Location" => "/next"
            },
        },
        ExpectedRequest {
            expected_method: HttpMethod::Get,
            expected_uri: "/next".into(),
            expected_body: "".into(),
            response_status: HttpStatus::OK,
            response_body: "real content".into(),
            response_headers: Default::default(),
        },
    ])
    .unwrap();

    let handle = std::thread::spawn(move || server.serve_one());
    let mut body = get(format!("http://localhost:{}/", port).as_ref()).unwrap();
    handle.join().unwrap().unwrap();

    let mut body_str = String::new();
    body.read_to_string(&mut body_str).unwrap();
}
