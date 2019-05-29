# http_io

Crate containing HTTP client and server.

- Designed to have limited dependencies, supports `#![no_std]`.
- Focus on streaming IO.
- Support for providing your own transport.

## Example

```rust
use http_io::error::Result;
use std::fs::File;
use std::io;

fn main() -> Result<()> {
    // Stream contents of url to stdout
    let mut body = http_io::client::get("http://www.google.com")?;
    io::copy(&mut body, &mut std::io::stdout())?;

    // Stream contents of file to remote server
    let file = File::open("src/client.rs")?;
    http_io::client::put("http://www.google.com", file)?;
    Ok(())
}
```
