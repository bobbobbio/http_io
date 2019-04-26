use std::io;

use http_io::error::Result;
use http_io::url::Url;

fn main() -> Result<()> {
    let args = std::env::args();
    let url: Url = args
        .skip(1)
        .next()
        .unwrap_or("http://www.google.com".into())
        .parse()?;
    io::copy(&mut http_io::client::get(url)?, &mut io::stdout())?;
    Ok(())
}
