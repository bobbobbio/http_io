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
//! use http_io::error::Result;
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
use crate::error::{Error, Result};
use crate::io;
use crate::protocol::{HttpBody, HttpMethod, HttpRequest, HttpResponse};
#[cfg(not(feature = "std"))]
use alloc::{boxed::Box, string::String};

/// Represents the ability to accept a new abstract connection.
pub trait Listen {
    type stream: io::Read + io::Write;
    fn accept(&self) -> Result<Self::stream>;
}

#[cfg(feature = "std")]
impl Listen for std::net::TcpListener {
    type stream = std::net::TcpStream;
    fn accept(&self) -> Result<std::net::TcpStream> {
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
    fn accept(&self) -> Result<Self::stream> {
        let stream = self.listener.accept()?;
        Ok(self.acceptor.accept(stream)?)
    }
}

/// Represents the ability to service and respond to HTTP requests.
pub trait HttpRequestHandler<I: io::Read> {
    fn get(&mut self, uri: String) -> Result<HttpResponse<Box<dyn io::Read>>>;
    fn put(
        &mut self,
        uri: String,
        stream: HttpBody<&mut I>,
    ) -> Result<HttpResponse<Box<dyn io::Read>>>;
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

    /// Accept one new HTTP stream and serve one request off it.
    pub fn serve_one(&mut self) -> Result<()> {
        let mut stream = self.connection_stream.accept()?;
        let request = HttpRequest::deserialize(io::BufReader::new(&mut stream))?;

        let mut response = match request.method {
            HttpMethod::Get => self.request_handler.get(request.uri)?,
            HttpMethod::Put => {
                if !request.body.has_length() {
                    return Err(Error::Other("Length Required".into()));
                }
                self.request_handler.put(request.uri, request.body)?
            }
        };

        response.serialize(&mut stream)?;
        io::copy(&mut response.body, &mut stream)?;

        Ok(())
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
use crate::protocol::HttpStatus;

#[cfg(test)]
#[derive(PartialEq, Debug)]
pub struct ExpectedRequest {
    pub expected_method: HttpMethod,
    pub expected_uri: String,

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
impl<I: io::Read> HttpRequestHandler<I> for TestRequestHandler {
    fn get(&mut self, uri: String) -> Result<HttpResponse<Box<dyn io::Read>>> {
        let request = self.script.remove(0);
        assert_eq!(request.expected_method, HttpMethod::Get);
        assert_eq!(request.expected_uri, uri);

        Ok(HttpResponse::new(
            request.response_status,
            Box::new(io::Cursor::new(
                request.response_body.into_boxed_str().into_boxed_bytes(),
            )),
        ))
    }

    fn put(
        &mut self,
        uri: String,
        _stream: HttpBody<&mut I>,
    ) -> Result<HttpResponse<Box<dyn io::Read>>> {
        let request = self.script.remove(0);
        assert_eq!(request.expected_method, HttpMethod::Put);
        assert_eq!(request.expected_uri, uri);

        Ok(HttpResponse::new(
            request.response_status,
            Box::new(io::Cursor::new(
                request.response_body.into_boxed_str().into_boxed_bytes(),
            )),
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
) -> Result<(u16, HttpServer<std::net::TcpListener, TestRequestHandler>)> {
    let server_socket = std::net::TcpListener::bind("localhost:0")?;
    let server_address = server_socket.local_addr()?;
    let handler = TestRequestHandler::new(script);
    let server = HttpServer::new(server_socket, handler);

    Ok((server_address.port(), server))
}

#[cfg(test)]
pub fn test_ssl_server(
    script: Vec<ExpectedRequest>,
) -> Result<(
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
