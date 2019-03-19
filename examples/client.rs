use std::io::{self, Read, Write};

use http_io::error::Result;

fn main() -> Result<()> {
    let mut args = std::env::args();
    let host = args.next().unwrap_or("www.google.com".into());
    let mut body = Vec::new();
    http_io::client::get(host, "/")?.read_to_end(&mut body)?;
    io::stdout().write(&body)?;
    Ok(())
}
