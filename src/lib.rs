//! An HTTP client and server with minimal dependencies.
//!
//! See the `client` module for HTTP client code.
//! See the `server` module for HTTP server code.
//! See the `url` module for code representing urls.
pub mod client;
pub mod server;

pub mod error;
pub mod protocol;
pub mod url;
