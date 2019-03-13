use std::convert;
use std::io::{self, Read, Write};
use std::net;

mod client;
mod error;
mod protocol;
mod server;

use self::client::HttpClient;
use self::server::HttpServer;

fn client_main(mut args: std::env::Args) {
    let host = args.next().unwrap_or("www.google.com".into());

    let s = net::TcpStream::connect((host.as_ref(), 80)).unwrap();
    let h = HttpClient::new(s);
    let (response, mut body_stream) = h.get(host, "/".into()).unwrap();

    let mut body = Vec::new();
    body_stream.read_to_end(&mut body).unwrap();

    println!("{:#?}", response);
    io::stdout().write(&body).unwrap();
}

fn server_main(_args: std::env::Args) {
    let socket = net::TcpListener::bind("127.0.0.1:8080").unwrap();
    let server = HttpServer::new(socket);
    println!("Server started on port 8080");
    server.serve_forever();
}

fn main() {
    let mut args = std::env::args();
    args.next();
    let command = args.next();
    match command.as_ref().map(convert::AsRef::as_ref) {
        Some("client") => client_main(args),
        Some("server") => server_main(args),
        _ => panic!("Bad arguments"),
    }
}
