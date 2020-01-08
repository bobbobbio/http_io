//! A very simple HTTP server. It is not suitable for production workloads.
//! Users should write their own request handler which implements the `HttpRequestHandler` trait.
//!
//! # File Server Example
//! ```rust
//! use std::io;
//! use std::net;
//! use std::path::PathBuf;
//! use std::thread;
//!
//! use http_io::error::{Error, Result};
//! use http_io::protocol::{HttpBody, HttpResponse, HttpStatus};
//! use http_io::server::{HttpRequestHandler, HttpServer};
//!
//! struct FileHandler {
//!     file_root: PathBuf,
//! }
//!
//! impl FileHandler {
//!     fn new<P: Into<PathBuf>>(file_root: P) -> Self {
//!         FileHandler {
//!             file_root: file_root.into(),
//!         }
//!     }
//! }
//!
//! impl<I: io::Read> HttpRequestHandler<I> for FileHandler {
//!     type Error = Error;
//!     fn get(
//!         &mut self,
//!         uri: String,
//!     ) -> Result<HttpResponse<Box<dyn io::Read>>> {
//!         let path = self.file_root.join(uri.trim_start_matches("/"));
//!         Ok(HttpResponse::new(
//!             HttpStatus::OK,
//!             Box::new(std::fs::File::open(path)?),
//!         ))
//!     }
//!
//!     fn put(
//!         &mut self,
//!         uri: String,
//!         mut stream: HttpBody<&mut I>,
//!     ) -> Result<HttpResponse<Box<dyn io::Read>>> {
//!         let path = self.file_root.join(uri.trim_start_matches("/"));
//!         let mut file = std::fs::File::create(path)?;
//!         io::copy(&mut stream, &mut file)?;
//!         Ok(HttpResponse::new(HttpStatus::OK, Box::new(io::empty())))
//!     }
//! }
//!
//! fn main() -> Result<()> {
//!     let socket = net::TcpListener::bind("127.0.0.1:0")?;
//!     let port = socket.local_addr()?.port();
//!     let handle: thread::JoinHandle<Result<()>> = thread::spawn(move || {
//!         let handler = FileHandler::new(std::env::current_dir()?);
//!         let mut server = HttpServer::new(socket, handler);
//!         server.serve_one()?;
//!         Ok(())
//!     });
//!
//!     let url = format!("http://localhost:{}/src/server.rs", port);
//!     let mut body = http_io::client::get(url.as_ref())?;
//!     io::copy(&mut body, &mut std::io::stdout())?;
//!     handle.join().unwrap()?;
//!
//!     Ok(())
//! }
//! ```
use crate::io;
use crate::protocol::{HttpBody, HttpMethod, HttpRequest, HttpResponse, HttpStatus};
#[cfg(not(feature = "std"))]
use alloc::{
    boxed::Box,
    string::{String, ToString},
};
use core::result::Result;

type HttpResult<T> = core::result::Result<T, HttpResponse<Box<dyn io::Read>>>;

impl From<crate::error::Error> for HttpResponse<Box<dyn io::Read>> {
    fn from(error: crate::error::Error) -> Self {
        match error {
            crate::error::Error::LengthRequired => {
                HttpResponse::from_string(HttpStatus::LengthRequired, "length required")
            }
            e => HttpResponse::from_string(HttpStatus::InternalServerError, e.to_string()),
        }
    }
}

/// Represents the ability to accept a new abstract connection.
pub trait Listen {
    type stream: io::Read + io::Write;
    fn accept(&self) -> crate::error::Result<Self::stream>;
}

#[cfg(feature = "std")]
impl Listen for std::net::TcpListener {
    type stream = std::net::TcpStream;
    fn accept(&self) -> crate::error::Result<std::net::TcpStream> {
        let (stream, _) = std::net::TcpListener::accept(self)?;
        Ok(stream)
    }
}

#[cfg(feature = "openssl")]
pub struct SslListener<L> {
    listener: L,
    acceptor: openssl::ssl::SslAcceptor,
}

#[cfg(feature = "openssl")]
impl<L: Listen> SslListener<L> {
    pub fn new(listener: L, acceptor: openssl::ssl::SslAcceptor) -> Self {
        Self { listener, acceptor }
    }
}

#[cfg(feature = "openssl")]
impl<L: Listen> Listen for SslListener<L>
where
    <L as Listen>::stream: std::fmt::Debug,
{
    type stream = openssl::ssl::SslStream<<L as Listen>::stream>;
    fn accept(&self) -> crate::error::Result<Self::stream> {
        let stream = self.listener.accept()?;
        Ok(self.acceptor.accept(stream)?)
    }
}

/// Represents the ability to service and respond to HTTP requests.
pub trait HttpRequestHandler<I: io::Read> {
    type Error: Into<HttpResponse<Box<dyn io::Read>>>;

    fn get(&mut self, _uri: String) -> Result<HttpResponse<Box<dyn io::Read>>, Self::Error> {
        Ok(HttpResponse::from_string(
            HttpStatus::MethodNotAllowed,
            "GET not allowed",
        ))
    }

    fn head(&mut self, _uri: String) -> Result<HttpResponse<Box<dyn io::Read>>, Self::Error> {
        Ok(HttpResponse::from_string(
            HttpStatus::MethodNotAllowed,
            "HEAD not allowed",
        ))
    }

    fn options(&mut self, _uri: String) -> Result<HttpResponse<Box<dyn io::Read>>, Self::Error> {
        Ok(HttpResponse::from_string(
            HttpStatus::MethodNotAllowed,
            "OPTIONS not allowed",
        ))
    }

    fn put(
        &mut self,
        _uri: String,
        _stream: HttpBody<&mut I>,
    ) -> Result<HttpResponse<Box<dyn io::Read>>, Self::Error> {
        Ok(HttpResponse::from_string(
            HttpStatus::MethodNotAllowed,
            "PUT not allowed",
        ))
    }

    fn post(
        &mut self,
        _uri: String,
        _stream: HttpBody<&mut I>,
    ) -> Result<HttpResponse<Box<dyn io::Read>>, Self::Error> {
        Ok(HttpResponse::from_string(
            HttpStatus::MethodNotAllowed,
            "PUT not allowed",
        ))
    }
}

/// A simple HTTP server. Not suited for production workloads, better used in tests and small
/// projects.
pub struct HttpServer<L: Listen, H: HttpRequestHandler<L::stream>> {
    connection_stream: L,
    request_handler: H,
}

impl<L: Listen, H: HttpRequestHandler<L::stream>> HttpServer<L, H> {
    pub fn new(connection_stream: L, request_handler: H) -> Self {
        HttpServer {
            connection_stream,
            request_handler,
        }
    }

    pub fn serve_one(&mut self) -> io::Result<()> {
        let mut stream = self.connection_stream.accept()?;
        let mut response = match self.serve_one_inner(&mut stream) {
            Ok(response) => response,
            Err(response) => response,
        };

        response.serialize(&mut stream)?;
        io::copy(&mut response.body, &mut stream)?;

        Ok(())
    }

    /// Accept one new HTTP stream and serve one request off it.
    pub fn serve_one_inner(
        &mut self,
        stream: &mut <L as Listen>::stream,
    ) -> HttpResult<HttpResponse<Box<dyn io::Read>>> {
        let request = HttpRequest::deserialize(io::BufReader::new(stream))?;

        match request.method {
            HttpMethod::Get => self.request_handler.get(request.uri),
            HttpMethod::Head => self.request_handler.head(request.uri),
            HttpMethod::Options => self.request_handler.options(request.uri),
            HttpMethod::Post => {
                request.body.require_length()?;
                self.request_handler.post(request.uri, request.body)
            }
            HttpMethod::Put => {
                request.body.require_length()?;
                self.request_handler.put(request.uri, request.body)
            }
        }
        .map_err(|e| e.into())
    }

    /// Run `serve_one` in a loop forever
    ///
    /// *This function is available if http_io is built with the `"std"` feature.*
    #[cfg(feature = "std")]
    pub fn serve_forever(&mut self) -> ! {
        loop {
            if let Err(e) = self.serve_one() {
                println!("Error {:?}", e)
            }
        }
    }
}

#[cfg(test)]
#[derive(PartialEq, Debug)]
pub struct ExpectedRequest {
    pub expected_method: HttpMethod,
    pub expected_uri: String,
    pub expected_body: String,

    pub response_status: HttpStatus,
    pub response_body: String,
}

#[cfg(test)]
pub struct TestRequestHandler {
    script: Vec<ExpectedRequest>,
}

#[cfg(test)]
impl TestRequestHandler {
    fn new(script: Vec<ExpectedRequest>) -> Self {
        Self { script }
    }
}

#[cfg(test)]
use std::io::Read;

#[cfg(test)]
impl<I: io::Read> HttpRequestHandler<I> for TestRequestHandler {
    type Error = HttpResponse<Box<dyn io::Read>>;

    fn get(&mut self, uri: String) -> Result<HttpResponse<Box<dyn io::Read>>, Self::Error> {
        let request = self.script.remove(0);
        assert_eq!(request.expected_method, HttpMethod::Get);
        assert_eq!(request.expected_uri, uri);

        Ok(HttpResponse::from_string(
            request.response_status,
            request.response_body,
        ))
    }

    fn put(
        &mut self,
        uri: String,
        mut stream: HttpBody<&mut I>,
    ) -> Result<HttpResponse<Box<dyn io::Read>>, Self::Error> {
        let request = self.script.remove(0);
        assert_eq!(request.expected_method, HttpMethod::Put);
        assert_eq!(request.expected_uri, uri);

        let mut body_string = String::new();
        stream.read_to_string(&mut body_string).unwrap();
        assert_eq!(request.expected_body, body_string);

        Ok(HttpResponse::from_string(
            request.response_status,
            request.response_body,
        ))
    }
}

#[cfg(test)]
impl Drop for TestRequestHandler {
    fn drop(&mut self) {
        assert_eq!(&self.script, &vec![]);
    }
}

#[cfg(test)]
pub fn test_server(
    script: Vec<ExpectedRequest>,
) -> crate::error::Result<(u16, HttpServer<std::net::TcpListener, TestRequestHandler>)> {
    let server_socket = std::net::TcpListener::bind("localhost:0")?;
    let server_address = server_socket.local_addr()?;
    let handler = TestRequestHandler::new(script);
    let server = HttpServer::new(server_socket, handler);

    Ok((server_address.port(), server))
}

#[cfg(test)]
pub fn test_ssl_server(
    script: Vec<ExpectedRequest>,
) -> crate::error::Result<(
    u16,
    HttpServer<SslListener<std::net::TcpListener>, TestRequestHandler>,
)> {
    use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};

    let server_socket = std::net::TcpListener::bind("localhost:0")?;
    let server_address = server_socket.local_addr()?;
    let handler = TestRequestHandler::new(script);

    let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    acceptor
        .set_private_key_file(manifest_dir.join("test_key.pem"), SslFiletype::PEM)
        .unwrap();
    acceptor
        .set_certificate_chain_file(manifest_dir.join("test_cert.pem"))
        .unwrap();
    acceptor.check_private_key().unwrap();

    let stream = SslListener::new(server_socket, acceptor.build());
    let server = HttpServer::new(stream, handler);

    Ok((server_address.port(), server))
}
