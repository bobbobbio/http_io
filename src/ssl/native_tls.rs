use super::{Error, Result};
use crate::server::Listen;
use std::{fmt, io};

#[cfg(test)]
fn read_test_cert(name: &str) -> Result<Vec<u8>> {
    use io::Read as _;

    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut file = std::fs::File::open(manifest_dir.join(name))?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub struct SslClientStream<Stream>(native_tls::TlsStream<Stream>);

impl<Stream: io::Read + io::Write + fmt::Debug + 'static> SslClientStream<Stream> {
    pub fn new(host: &str, stream: Stream) -> Result<Self> {
        #[allow(unused_mut)]
        let mut builder = native_tls::TlsConnector::builder();

        #[cfg(test)]
        builder.add_root_certificate(native_tls::Certificate::from_pem(&read_test_cert(
            "test_ca.pem",
        )?)?);

        let connector = builder.build()?;
        Ok(Self(connector.connect(host, stream)?))
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

pub struct SslServerStream<Stream>(native_tls::TlsStream<Stream>);

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
    acceptor: native_tls::TlsAcceptor,
}

impl<L: Listen> SslListener<L> {
    pub fn new(private_key_pem: &[u8], cert_pem: &[u8], listener: L) -> Result<Self> {
        let identity = native_tls::Identity::from_pkcs8(cert_pem, private_key_pem)?;
        let acceptor = native_tls::TlsAcceptor::new(identity)?;
        Ok(Self { listener, acceptor })
    }
}

impl<L: Listen> Listen for SslListener<L>
where
    <L as Listen>::Stream: fmt::Debug + 'static,
{
    type Stream = SslServerStream<<L as Listen>::Stream>;

    fn accept(&self) -> crate::error::Result<Self::Stream> {
        let stream = self.listener.accept()?;
        Ok(SslServerStream(
            self.acceptor.accept(stream).map_err(|e| Error::from(e))?,
        ))
    }
}

// This required 'static bound here is super weird
impl<Stream: fmt::Debug + 'static> From<native_tls::HandshakeError<Stream>> for Error {
    fn from(e: native_tls::HandshakeError<Stream>) -> Self {
        Self(e.to_string())
    }
}

impl From<native_tls::Error> for Error {
    fn from(e: native_tls::Error) -> Self {
        Self(e.to_string())
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self(e.to_string())
    }
}
