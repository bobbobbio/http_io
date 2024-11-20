// disable false positive of not using the String, the dead code analysis
// intentionally ignore we derive the Debug trait
#[allow(dead_code)]
#[derive(Debug)]
pub struct Error(String);

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(feature = "openssl")]
#[path = "openssl.rs"]
mod inner;

#[cfg(feature = "rustls")]
#[path = "rustls.rs"]
mod inner;

#[cfg(feature = "native-tls")]
#[path = "native_tls.rs"]
mod inner;

pub use inner::*;
