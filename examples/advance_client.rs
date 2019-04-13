use std::io;
use std::net;

use http_io::client::HttpRequestBuilder;
use http_io::error::Result;
use http_io::url::Url;

fn main() -> Result<()> {
    let args = std::env::args();
    let url: Url = args
        .skip(1)
        .next()
        .unwrap_or("http://www.google.com".into())
        .parse()?;

    let s = net::TcpStream::connect((url.authority.as_ref(), url.port.unwrap_or(80)))?;
    let mut response = HttpRequestBuilder::get(url)?.send(s)?.finish()?;

    println!("{:#?}", response.headers);
    io::copy(&mut response.body, &mut io::stdout())?;
    Ok(())
}
