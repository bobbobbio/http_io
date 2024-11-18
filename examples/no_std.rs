///! This example uses the library as you would in a `no_std` environment, but we are of course
///! using `std`.
///!
///! This doesn't do actual HTTP requests, instead it does something fake to show how you might use
///! it to hook stuff up to your own sockets
use http_io::error::Result;

#[allow(dead_code)]
const CANNED_RESPONSE: &'static [u8] = b"\
HTTP/1.1 200 OK\r\n\
Content-Length: 11\r\n\
Content-Type: text/html\r\n\
Connection: Closed\r\n\
\r\n\
hello world
";

#[cfg(not(feature = "std"))]
mod no_std {

    use clap::Parser;
    use http_io::client::HttpRequestBuilder;
    use http_io::error::{Error, Result};
    use http_io::io;
    use http_io::protocol::HttpMethod;
    use http_io::url::HttpUrl;

    #[derive(Parser)]
    struct Options {
        #[clap(long = "method", default_value = "GET")]
        method: String,
        #[clap(long = "data", default_value = "")]
        data: String,
        url: String,
    }

    #[derive(Debug)]
    struct MyFakeTcpStream {
        #[allow(dead_code)]
        host: String,
        #[allow(dead_code)]
        port: u16,

        data: Vec<u8>,
    }

    impl MyFakeTcpStream {
        fn connect(host: &str, port: u16) -> io::Result<Self> {
            println!("fake connection: connect {host:?}:{port:?}");
            Ok(Self {
                host: host.into(),
                port,
                data: super::CANNED_RESPONSE.into(),
            })
        }
    }

    impl io::Read for MyFakeTcpStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let read_length = std::cmp::min(self.data.len(), buf.len());

            let remaining = self.data.split_off(read_length);

            let dst_slice = &mut buf[..read_length];
            dst_slice.clone_from_slice(&self.data[..]);
            println!("fake connection: server({self:?}) --> client: {dst_slice:?}");

            self.data = remaining;

            Ok(dst_slice.len())
        }
    }

    impl io::Write for MyFakeTcpStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            println!("fake connection: client --> server({self:?}): {buf:?}");
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            println!("fake connection: flush");
            Ok(())
        }
    }

    struct Stdout(std::io::Stdout);

    impl Stdout {
        fn new() -> Self {
            Self(std::io::stdout())
        }
    }

    impl io::Write for Stdout {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(std::io::Write::write(&mut self.0, buf).unwrap())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(std::io::Write::flush(&mut self.0).unwrap())
        }
    }

    pub fn main() -> Result<()> {
        let opts = Options::parse();

        // The clap parsing stuff relies on std::error::Error
        let method: HttpMethod = opts.method.parse()?;
        let url: HttpUrl = opts.url.parse()?;

        let s = MyFakeTcpStream::connect(url.host(), url.port()?)?;

        let mut body = match method {
            HttpMethod::Get => HttpRequestBuilder::get(url)?.send(s)?.finish()?.body,
            HttpMethod::Put => {
                let mut request = HttpRequestBuilder::put(url)?.send(s)?;
                io::copy(&mut opts.data.as_bytes(), &mut request)?;
                request.finish()?.body
            }
            m => return Err(Error::UnexpectedMethod(m)),
        };
        io::copy(&mut body, &mut Stdout::new())?;
        Ok(())
    }
}

#[cfg(feature = "std")]
fn main() -> Result<()> {
    panic!("must be compiled without the std feature");
}

#[cfg(not(feature = "std"))]
fn main() -> Result<()> {
    no_std::main()
}
