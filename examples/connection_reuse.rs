use std::io::{self, Read, Write};

use http_io::client::HttpClient;
use http_io::error::Result;

fn main() -> Result<()> {
    let args = std::env::args();
    let host = args.skip(1).next().unwrap_or("www.google.com".into());

    let mut client = HttpClient::<std::net::TcpStream>::new();
    for uri in &["/", "/favicon.ico", "/robots.txt"] {
        let mut body = Vec::new();
        client
            .get(&host, uri)?
            .finish()?
            .body
            .read_to_end(&mut body)?;
        io::stdout().write(&body)?;
    }

    Ok(())
}
