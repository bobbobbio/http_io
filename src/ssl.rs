use crate::server::Listen;
use std::fmt;
use std::net::TcpStream;

#[derive(Debug)]
pub struct Error(String);

pub type Result<T> = std::result::Result<T, Error>;

pub type SslTransport<T> = openssl::ssl::SslStream<T>;

pub fn ssl_stream(host: &str, stream: TcpStream) -> Result<SslTransport<TcpStream>> {
    use openssl::ssl::{Ssl, SslContext, SslMethod, SslVerifyMode};

    let mut ctx = SslContext::builder(SslMethod::tls())?;
    ctx.set_default_verify_paths()?;

    #[cfg(test)]
    {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        ctx.set_ca_file(manifest_dir.join("test_cert.pem"))?;
        ctx.set_ca_file(manifest_dir.join("test_bad_cert.pem"))?;
    }

    ctx.set_verify(SslVerifyMode::PEER);

    let mut ssl = Ssl::new(&ctx.build())?;
    ssl.param_mut().set_host(host)?;
    ssl.set_hostname(host)?;
    Ok(ssl.connect(stream)?)
}

pub struct SslListener<L> {
    listener: L,
    acceptor: openssl::ssl::SslAcceptor,
}

impl<L: Listen> SslListener<L> {
    pub fn new(key_file: &str, cert_file: &str, listener: L) -> Result<Self> {
        use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
        use std::path::PathBuf;

        let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        acceptor.set_private_key_file(manifest_dir.join(key_file), SslFiletype::PEM)?;
        acceptor.set_certificate_chain_file(manifest_dir.join(cert_file))?;
        acceptor.check_private_key()?;

        Ok(Self {
            listener,
            acceptor: acceptor.build(),
        })
    }
}

impl<L: Listen> Listen for SslListener<L>
where
    <L as Listen>::Stream: fmt::Debug,
{
    type Stream = openssl::ssl::SslStream<<L as Listen>::Stream>;

    fn accept(&self) -> crate::error::Result<Self::Stream> {
        let stream = self.listener.accept()?;
        Ok(self.acceptor.accept(stream).map_err(|e| Error::from(e))?)
    }
}

impl From<openssl::error::ErrorStack> for Error {
    fn from(e: openssl::error::ErrorStack) -> Self {
        Error(e.to_string())
    }
}

impl<S: fmt::Debug> From<openssl::ssl::HandshakeError<S>> for Error {
    fn from(e: openssl::ssl::HandshakeError<S>) -> Self {
        Error(e.to_string())
    }
}
