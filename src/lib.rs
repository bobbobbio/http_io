//! An HTTP client and server with minimal dependencies.
//!
//! See the `client` module for HTTP client code.
//! See the `server` module for HTTP server code.
//! See the `url` module for code representing urls.
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

pub mod client;
pub mod server;

pub mod error;
pub mod protocol;
pub mod url;

#[cfg(not(feature = "std"))]
mod io;

#[cfg(not(feature = "std"))]
pub use io::{Read, Write};

#[cfg(feature = "std")]
use std::io;
