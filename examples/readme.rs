use http_io::error::Result;
use std::fs::File;
use std::io;

fn main() -> Result<()> {
    // Stream contents of url to stdout
    let mut body = http_io::client::get("https://postman-echo.com/get")?;
    io::copy(&mut body, &mut std::io::stdout())?;

    // Stream contents of file to remote server
    let file = File::open("src/client.rs")?;
    http_io::client::put("https://postman-echo.com/put", file)?;
    Ok(())
}
