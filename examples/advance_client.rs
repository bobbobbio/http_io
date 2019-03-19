use std::io;
use std::net;

use http_io::client::HttpRequestBuilder;
use http_io::error::Result;

fn main() -> Result<()> {
    let mut args = std::env::args();
    let host = args.next().unwrap_or("www.google.com".into());

    let s = net::TcpStream::connect((host.as_ref(), 80))?;
    let mut response = HttpRequestBuilder::get(host, "/").send(s)?.finish()?;

    println!("{:#?}", response.headers);
    io::copy(&mut response.body, &mut io::stdout())?;
    Ok(())
}
