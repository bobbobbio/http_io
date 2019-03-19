use std::io::{self, Read, Write};
use std::net;

use http_io::client::HttpClient;
use http_io::error::Result;

fn main() -> Result<()> {
    let mut args = std::env::args();
    let host = args.next().unwrap_or("www.google.com".into());

    let s = net::TcpStream::connect((host.as_ref(), 80))?;
    let h = HttpClient::new(s);
    let mut response = h.get(host, "/")?;

    let mut body = Vec::new();
    response.body.read_to_end(&mut body)?;

    println!("{:#?}", response.headers);
    io::stdout().write(&body)?;
    Ok(())
}
