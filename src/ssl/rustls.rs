use super::{Error, Result};
use crate::io;
use crate::server::Listen;
use std::convert::TryInto as _;

#[cfg(feature = "std")]
use std::sync::Arc;

#[cfg(not(feature = "std"))]
use alloc::sync::Arc;

#[cfg(test)]
fn read_test_cert(name: &str) -> Result<Vec<u8>> {
    use io::Read as _;

    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut file = std::fs::File::open(manifest_dir.join(name))?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn root_store() -> Result<rustls::RootCertStore> {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
        rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
            ta.subject,
            ta.spki,
            ta.name_constraints,
        )
    }));

    #[cfg(test)]
    for c in rustls_pemfile::certs(&mut io::BufReader::new(&read_test_cert("test_ca.pem")?[..]))?
        .iter()
        .map(|v| rustls::Certificate(v.clone()))
    {
        root_store.add(&c).map_err(|e| Error(e.to_string()))?;
    }

    Ok(root_store)
}

pub struct SslClientStream<Stream: io::Read + io::Write>(
    rustls::StreamOwned<rustls::ClientConnection, Stream>,
);

impl<Stream: io::Read + io::Write> SslClientStream<Stream> {
    pub fn new(host: &str, mut stream: Stream) -> Result<Self> {
        let config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store()?)
            .with_no_client_auth();
        assert!(config.enable_sni);

        let server_name = host.try_into()?;
        let mut conn = rustls::ClientConnection::new(Arc::new(config), server_name)?;

        'outer: while conn.is_handshaking() {
            while conn.wants_write() {
                assert_ne!(conn.write_tls(&mut stream)?, 0);
            }

            while conn.is_handshaking() && conn.wants_read() {
                if conn.read_tls(&mut stream)? == 0 {
                    break 'outer;
                }
                conn.process_new_packets()?;
            }
        }

        if conn.is_handshaking() {
            return Err(Error("SSL handshake failed".into()));
        }

        Ok(Self(rustls::StreamOwned::new(conn, stream)))
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

pub struct SslServerStream<Stream: io::Read + io::Write>(
    rustls::StreamOwned<rustls::ServerConnection, Stream>,
);

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
    config: Arc<rustls::ServerConfig>,
}

impl<L: Listen> SslListener<L> {
    pub fn new(private_key_pem: &[u8], cert_pem: &[u8], listener: L) -> Result<Self> {
        let private_key = rustls::PrivateKey(
            rustls_pemfile::pkcs8_private_keys(&mut io::BufReader::new(private_key_pem))?[0]
                .clone(),
        );
        let certs = rustls_pemfile::certs(&mut io::BufReader::new(cert_pem))?
            .iter()
            .map(|v| rustls::Certificate(v.clone()))
            .collect();

        let config = rustls::ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certs, private_key)?;

        Ok(Self {
            listener,
            config: Arc::new(config),
        })
    }

    fn get_conn_from_stream(
        &self,
        mut stream: impl io::Read + io::Write,
    ) -> Result<rustls::ServerConnection> {
        let mut acceptor = rustls::server::Acceptor::new()?;
        acceptor.read_tls(&mut stream)?;
        let accepted = acceptor
            .accept()?
            .ok_or(Error("failed to accept TLS connection".into()))?;
        let mut conn = accepted.into_connection(self.config.clone())?;

        'outer: while conn.is_handshaking() {
            while conn.is_handshaking() && conn.wants_read() {
                if conn.read_tls(&mut stream)? == 0 {
                    break 'outer;
                }
                conn.process_new_packets()?;
            }

            while conn.wants_write() {
                assert_ne!(conn.write_tls(&mut stream)?, 0);
            }
        }

        if conn.is_handshaking() {
            return Err(Error("SSL handshake failed".into()));
        }

        Ok(conn)
    }
}

impl<L: Listen> Listen for SslListener<L> {
    type Stream = SslServerStream<<L as Listen>::Stream>;

    fn accept(&self) -> crate::error::Result<Self::Stream> {
        let mut stream = self.listener.accept()?;
        let conn = self
            .get_conn_from_stream(&mut stream)
            .map_err(|e| Error::from(e))?;
        Ok(SslServerStream(rustls::StreamOwned::new(conn, stream)))
    }
}

impl From<rustls::client::InvalidDnsNameError> for Error {
    fn from(e: rustls::client::InvalidDnsNameError) -> Self {
        Self(e.to_string())
    }
}

impl From<rustls::Error> for Error {
    fn from(e: rustls::Error) -> Self {
        Self(e.to_string())
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self(e.to_string())
    }
}
