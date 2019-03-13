use std::convert;
use std::io::{self, Read, Write};
use std::net;

use http_io::client::HttpClient;
use http_io::error::Result;
use http_io::server::HttpServer;

fn client_main(mut args: std::env::Args) -> Result<()> {
    let host = args.next().unwrap_or("www.google.com".into());

    let s = net::TcpStream::connect((host.as_ref(), 80))?;
    let h = HttpClient::new(s);
    let (response, mut body_stream) = h.get(host, "/")?;

    let mut body = Vec::new();
    body_stream.read_to_end(&mut body)?;

    println!("{:#?}", response);
    io::stdout().write(&body)?;
    Ok(())
}

fn simple_client_main(mut args: std::env::Args) -> Result<()> {
    let host = args.next().unwrap_or("www.google.com".into());
    let mut body = Vec::new();
    http_io::client::get(host, "/")?.read_to_end(&mut body)?;
    io::stdout().write(&body)?;
    Ok(())
}

fn server_main(_args: std::env::Args) -> Result<()> {
    let socket = net::TcpListener::bind("127.0.0.1:8080")?;
    let server = HttpServer::new(socket);
    println!("Server started on port 8080");
    server.serve_forever();
}

fn main() -> Result<()> {
    let mut args = std::env::args();
    args.next();
    let command = args.next();
    match command.as_ref().map(convert::AsRef::as_ref) {
        Some("client") => client_main(args),
        Some("simple_client") => simple_client_main(args),
        Some("server") => server_main(args),
        _ => panic!("Bad arguments"),
    }
}
