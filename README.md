# http_io [![Latest Version]][crates.io]

[Latest Version]: https://img.shields.io/crates/v/http_io.svg
[crates.io]: https://crates.io/crates/http_io

Crate containing HTTP client and server.

- Designed to have limited dependencies, supports `#![no_std]`.
- Focus on streaming IO.
- Support for providing your own transport.
- Supports HTTPS

The no_std build requires nightly since it relies on the alloc crate.

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

## Choosing an TLS backend

By default `http_io` uses [`native-tls`](https://crates.io/crates/native-tls) as its library for TLS (HTTP support). It supports two other TLS libraries, [`rustls`](https://crates.io/crates/rustls) and [`openssl`](https://crates.io/crates/openssl). These other "back-ends" can be selected using feaures

```bash
$ # If you want to use `rustls`:
$ cargo build --no-default-features --features std,ssl-rustls
$ # If you want to use `openssl`:
$ cargo build --no-default-features --features std,ssl-rustls
```
