use std::io::{self, Read, Write};

use http_io::error::Result;
use http_io::url::Url;

fn main() -> Result<()> {
    let args = std::env::args();
    let url: Url = args
        .skip(1)
        .next()
        .unwrap_or("http://www.google.com".into())
        .parse()?;
    let mut body = Vec::new();
    http_io::client::get(url)?.read_to_end(&mut body)?;
    io::stdout().write(&body)?;
    Ok(())
}
