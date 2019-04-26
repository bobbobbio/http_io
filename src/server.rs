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
//!         &self,
//!         uri: String,
//!         _stream: HttpBody<&mut I>,
//!     ) -> Result<HttpResponse<Box<dyn io::Read>>> {
//!         let path = self.file_root.join(uri.trim_start_matches("/"));
//!         Ok(HttpResponse::new(
//!             HttpStatus::OK,
//!             Box::new(std::fs::File::open(path)?),
//!         ))
//!     }
//!
//!     fn put(
//!         &self,
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
//!     let handle: thread::JoinHandle<Result<()>> = thread::spawn(|| {
//!         let handler = FileHandler::new(std::env::current_dir()?);
//!         let socket = net::TcpListener::bind("127.0.0.1:8080")?;
//!         let server = HttpServer::new(socket, handler);
//!         server.serve_one()?;
//!         Ok(())
//!     });
//!
//!     let mut body = http_io::client::get("http://localhost:8080/src/server.rs")?;
//!     io::copy(&mut body, &mut std::io::stdout())?;
//!     handle.join().unwrap()?;
//!
//!     Ok(())
//! }
//! ```
use crate::error::Result;
use crate::protocol::{HttpBody, HttpMethod, HttpRequest, HttpResponse};
use std::io;
use std::net;

/// Represents the ability to accept a new abstract connection.
pub trait Listen {
    type stream: io::Read + io::Write;
    fn accept(&self) -> Result<Self::stream>;
}

impl Listen for net::TcpListener {
    type stream = net::TcpStream;
    fn accept(&self) -> Result<net::TcpStream> {
        let (stream, _) = net::TcpListener::accept(self)?;
        Ok(stream)
    }
}

/// Represents the ability to service and respond to HTTP requests.
pub trait HttpRequestHandler<I: io::Read> {
    fn get(&self, uri: String, stream: HttpBody<&mut I>)
        -> Result<HttpResponse<Box<dyn io::Read>>>;
    fn put(&self, uri: String, stream: HttpBody<&mut I>)
        -> Result<HttpResponse<Box<dyn io::Read>>>;
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
    pub fn serve_one(&self) -> Result<()> {
        let mut stream = self.connection_stream.accept()?;
        let request = HttpRequest::deserialize(io::BufReader::new(&mut stream))?;

        let mut response = match request.method {
            HttpMethod::Get => self.request_handler.get(request.uri, request.body)?,
            HttpMethod::Put => self.request_handler.put(request.uri, request.body)?,
        };

        response.serialize(&mut stream)?;
        io::copy(&mut response.body, &mut stream)?;

        Ok(())
    }

    /// Run `serve_one` in a loop forever
    pub fn serve_forever(&self) -> ! {
        loop {
            match self.serve_one() {
                Err(e) => println!("Error {:?}", e),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod client_server_tests {
    use super::{HttpRequestHandler, HttpServer};
    use crate::client::HttpRequestBuilder;
    use crate::error::Result;
    use crate::protocol::{HttpBody, HttpResponse, HttpStatus};
    use std::{io, net, thread};

    struct TestRequestHandler();

    impl TestRequestHandler {
        fn new() -> Self {
            TestRequestHandler()
        }
    }

    impl<I: io::Read> HttpRequestHandler<I> for TestRequestHandler {
        fn get(
            &self,
            _uri: String,
            _stream: HttpBody<&mut I>,
        ) -> Result<HttpResponse<Box<dyn io::Read>>> {
            Ok(HttpResponse::new(HttpStatus::OK, Box::new(io::empty())))
        }
        fn put(
            &self,
            _uri: String,
            _stream: HttpBody<&mut I>,
        ) -> Result<HttpResponse<Box<dyn io::Read>>> {
            Ok(HttpResponse::new(HttpStatus::OK, Box::new(io::empty())))
        }
    }

    fn connected_client_server() -> Result<(
        net::TcpStream,
        HttpServer<net::TcpListener, TestRequestHandler>,
    )> {
        let server_socket = net::TcpListener::bind("localhost:0")?;
        let server_address = server_socket.local_addr()?;
        let handler = TestRequestHandler::new();
        let server = HttpServer::new(server_socket, handler);

        let client_socket = net::TcpStream::connect(server_address)?;

        Ok((client_socket, server))
    }

    #[test]
    fn request_one() -> Result<()> {
        let (client_socket, server) = connected_client_server()?;
        let handle = thread::spawn(move || server.serve_one());
        let response = HttpRequestBuilder::get("http://localhost/")?
            .send(client_socket)?
            .finish()?;
        handle.join().unwrap()?;
        assert_eq!(response.status, HttpStatus::OK);
        Ok(())
    }
}
