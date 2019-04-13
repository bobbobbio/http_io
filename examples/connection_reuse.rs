use std::io::{self, Read, Write};

use http_io::client::HttpClient;
use http_io::error::Result;
use http_io::url::Url;

fn main() -> Result<()> {
    let args = std::env::args();
    let url: Url = args
        .skip(1)
        .next()
        .unwrap_or("http://www.google.com".into())
        .parse()?;

    let mut client = HttpClient::<std::net::TcpStream>::new();
    for path in &["/", "/favicon.ico", "/robots.txt"] {
        let mut url = url.clone();
        url.path = path.parse()?;
        let mut body = Vec::new();
        client.get(url)?.finish()?.body.read_to_end(&mut body)?;
        io::stdout().write(&body)?;
    }

    Ok(())
}
