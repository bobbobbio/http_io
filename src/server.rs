use crate::error::Result;
use crate::protocol::{CrLfStream, HttpRequest, HttpResponse, HttpStatus};
use std::io;
use std::io::Write;
use std::net;

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

pub struct HttpServer<L: Listen> {
    connection_stream: L,
}

impl<L: Listen> HttpServer<L> {
    pub fn new(connection_stream: L) -> Self {
        HttpServer { connection_stream }
    }

    fn serve_one(&self) -> Result<()> {
        let mut stream = io::BufReader::new(self.connection_stream.accept()?);
        let mut ts = CrLfStream::new(&mut stream);
        let request = HttpRequest::deserialize(&mut ts)?;

        let mut stream = stream.into_inner();
        let response = HttpResponse::new(HttpStatus::OK);
        write!(stream, "{}{:#?}", response, request)?;
        println!("Served one request");

        Ok(())
    }

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
    use super::HttpServer;
    use crate::client::HttpClient;
    use crate::protocol::HttpStatus;
    use std::net;
    use std::thread;

    fn connected_client_server() -> (HttpClient<net::TcpStream>, HttpServer<net::TcpListener>) {
        let server_socket = net::TcpListener::bind("localhost:0").unwrap();
        let server_address = server_socket.local_addr().unwrap();
        let server = HttpServer::new(server_socket);

        let client_socket = net::TcpStream::connect(server_address).unwrap();
        let client = HttpClient::new(client_socket);

        (client, server)
    }

    #[test]
    fn request_one() {
        let (client, server) = connected_client_server();
        let handle = thread::spawn(move || server.serve_one());
        let (response, _body) = client.get("localhost", "/").unwrap();
        handle.join().unwrap().unwrap();
        assert_eq!(response.status(), HttpStatus::OK);
    }
}
