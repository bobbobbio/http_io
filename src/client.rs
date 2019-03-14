use crate::error::{Error, Result};
use crate::protocol::{self, CrLfStream, HttpHeaders, HttpMethod, HttpRequest, HttpStatus};
use std::io;
use std::io::Read;

pub struct HttpClient<S: io::Read + io::Write> {
    socket: S,
}

struct HttpBodyChunk<S: io::Read> {
    inner: io::Take<HttpReadTilCloseBody<S>>,
}

pub struct HttpChunkedBody<S: io::Read> {
    stream: Option<HttpReadTilCloseBody<S>>,
    chunk: Option<HttpBodyChunk<S>>,
}

impl<S: io::Read> HttpChunkedBody<S> {
    fn new(stream: HttpReadTilCloseBody<S>) -> Self {
        HttpChunkedBody {
            stream: Some(stream),
            chunk: None,
        }
    }
}

impl<S: io::Read> HttpBodyChunk<S> {
    fn new(mut stream: HttpReadTilCloseBody<S>) -> Result<Option<Self>> {
        let mut ts = CrLfStream::new(&mut stream);
        let size_str = ts.expect_next()?;
        drop(ts);
        let size = u64::from_str_radix(&size_str, 16)?;
        Ok(if size == 0 {
            None
        } else {
            Some(HttpBodyChunk {
                inner: stream.take(size),
            })
        })
    }

    fn into_inner(self) -> HttpReadTilCloseBody<S> {
        self.inner.into_inner()
    }
}

impl<S: io::Read> io::Read for HttpBodyChunk<S> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buffer)
    }
}

impl<S: io::Read> io::Read for HttpChunkedBody<S> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if let Some(mut chunk) = self.chunk.take() {
            let read = chunk.read(buffer)?;
            if read == 0 {
                let mut stream = chunk.into_inner();
                let mut b = [0; 2];
                stream.read(&mut b)?;
                self.stream = Some(stream);
                self.read(buffer)
            } else {
                self.chunk.replace(chunk);
                Ok(read)
            }
        } else if let Some(stream) = self.stream.take() {
            let new_chunk = HttpBodyChunk::new(stream)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            match new_chunk {
                Some(chunk) => {
                    self.chunk = Some(chunk);
                    self.read(buffer)
                }
                None => Ok(0),
            }
        } else {
            Ok(0)
        }
    }
}

#[cfg(test)]
mod chunked_encoding_tests {
    use super::HttpChunkedBody;
    use crate::error::Result;
    use std::io;
    use std::io::Read;

    fn chunk_test(i: &'static str) -> Result<String> {
        let input = io::BufReader::new(io::Cursor::new(i));
        let mut body = HttpChunkedBody::new(input);

        let mut output = String::new();
        body.read_to_string(&mut output)?;
        Ok(output)
    }

    #[test]
    fn simple_chunk() {
        assert_eq!(
            &chunk_test("a\r\n0123456789\r\n0\r\n").unwrap(),
            "0123456789"
        );
    }

    #[test]
    fn chunk_missing_last_chunk() {
        assert!(chunk_test("a\r\n0123456789\r\n").is_err());
    }

    #[test]
    fn chunk_short_read() {
        assert!(chunk_test("a\r\n012345678").is_err());
    }
}

type HttpReadTilCloseBody<S> = io::BufReader<S>;
type HttpLimitedBody<S> = io::Take<HttpReadTilCloseBody<S>>;

pub enum HttpBody<S: io::Read> {
    Chunked(HttpChunkedBody<S>),
    Limited(HttpLimitedBody<S>),
    ReadTilClose(HttpReadTilCloseBody<S>),
}

pub struct HttpResponse<S: io::Read> {
    pub status: HttpStatus,
    pub headers: HttpHeaders,
    pub body: HttpBody<S>,
}

impl<S: io::Read> HttpResponse<S> {
    fn new(status: HttpStatus, headers: HttpHeaders, body: HttpBody<S>) -> Self {
        HttpResponse {
            status,
            headers,
            body,
        }
    }
}

impl<S: io::Read> io::Read for HttpBody<S> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            HttpBody::Chunked(i) => i.read(buffer),
            HttpBody::Limited(i) => i.read(buffer),
            HttpBody::ReadTilClose(i) => i.read(buffer),
        }
    }
}

impl<S: io::Read + io::Write> HttpClient<S> {
    pub fn new(socket: S) -> HttpClient<S> {
        HttpClient { socket }
    }

    pub fn get<S1: AsRef<str>, S2: AsRef<str>>(
        mut self,
        host: S1,
        uri: S2,
    ) -> Result<HttpResponse<S>> {
        let mut request = HttpRequest::new(HttpMethod::Get, uri.as_ref());
        request.add_header("Host", host.as_ref());
        request.add_header("User-Agent", "fuck/bitches");
        request.add_header("Accept", "*/*");
        write!(self.socket, "{}", request)?;

        let mut stream = CrLfStream::new(&mut self.socket);
        let response = protocol::HttpResponse::deserialize(&mut stream)?;
        drop(stream);

        let body = io::BufReader::new(self.socket);

        let encoding = response.get_header("Transfer-Encoding");
        let content_length = response.get_header("Content-Length").map(str::parse);

        let body = if encoding == Some("chunked") {
            HttpBody::Chunked(HttpChunkedBody::new(body))
        } else if let Some(length) = content_length {
            HttpBody::Limited(body.take(length?))
        } else {
            HttpBody::ReadTilClose(body)
        };

        Ok(HttpResponse::new(response.status, response.headers, body))
    }
}

pub fn get<S1: AsRef<str>, S2: AsRef<str>>(
    host: S1,
    uri: S2,
) -> Result<HttpBody<std::net::TcpStream>> {
    let s = std::net::TcpStream::connect((host.as_ref(), 80))?;
    let c = HttpClient::new(s);
    let response = c.get(host, uri.as_ref())?;

    if response.status != HttpStatus::OK {
        return Err(Error::UnexpectedStatus(response.status));
    }

    Ok(response.body)
}
