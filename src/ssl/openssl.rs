use super::{Error, Result};
use crate::server::Listen;
use std::{fmt, io};

pub struct SslClientStream<Stream>(openssl::ssl::SslStream<Stream>);

impl<Stream: io::Read + io::Write + fmt::Debug> SslClientStream<Stream> {
    pub fn new(host: &str, stream: Stream) -> Result<Self> {
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
        Ok(Self(ssl.connect(stream)?))
    }
}

impl<Stream: io::Read + io::Write> io::Read for SslClientStream<Stream> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }

    fn read_vectored(&mut self, bufs: &mut [io::IoSliceMut<'_>]) -> io::Result<usize> {
        self.0.read_vectored(bufs)
    }
}

impl<Stream: io::Read + io::Write> io::Write for SslClientStream<Stream> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }

    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.0.write_vectored(bufs)
    }
}

pub struct SslServerStream<Stream>(openssl::ssl::SslStream<Stream>);

impl<Stream: io::Read + io::Write> io::Read for SslServerStream<Stream> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }

    fn read_vectored(&mut self, bufs: &mut [io::IoSliceMut<'_>]) -> io::Result<usize> {
        self.0.read_vectored(bufs)
    }
}

impl<Stream: io::Read + io::Write> io::Write for SslServerStream<Stream> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }

    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.0.write_vectored(bufs)
    }
}

pub struct SslListener<L> {
    listener: L,
    acceptor: openssl::ssl::SslAcceptor,
}

impl<L: Listen> SslListener<L> {
    pub fn new(private_key_pem: &[u8], cert_pem: &[u8], listener: L) -> Result<Self> {
        use openssl::pkey::PKey;
        use openssl::ssl::{SslAcceptor, SslMethod};
        use openssl::x509::X509;

        let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
        acceptor.set_private_key(PKey::private_key_from_pem(private_key_pem)?.as_ref())?;
        acceptor.set_certificate(X509::from_pem(cert_pem)?.as_ref())?;

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
    type Stream = SslServerStream<<L as Listen>::Stream>;

    fn accept(&self) -> crate::error::Result<Self::Stream> {
        let stream = self.listener.accept()?;
        Ok(SslServerStream(
            self.acceptor.accept(stream).map_err(|e| Error::from(e))?,
        ))
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
