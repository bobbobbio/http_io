use std::io;
use std::net;

use http_io::client::HttpRequestBuilder;
use http_io::error::Result;
use http_io::url::HttpUrl;

fn main() -> Result<()> {
    let args = std::env::args();
    let url: HttpUrl = args
        .skip(1)
        .next()
        .unwrap_or("http://www.google.com".into())
        .parse()?;

    let s = net::TcpStream::connect((url.host(), url.port()))?;
    let mut response = HttpRequestBuilder::get(url)?.send(s)?.finish()?;

    println!("{:#?}", response.headers);
    io::copy(&mut response.body, &mut io::stdout())?;
    Ok(())
}
