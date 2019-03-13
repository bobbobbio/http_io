# http_io

Crate containing HTTP client and server. Designed to have limited dependencies, and eventually support `#![no_std]`.

```rust
fn main -> http_io::error::Result<()> {
    let mut body = Vec::new();
    http_io::client::get("www.google.com", "/")?.read_to_end(&mut body)?;
    io::stdout().write(&body)?;
}
```
