use crate::error::{Error, Result};
use crate::protocol::{HttpBody, HttpMethod, HttpRequest, HttpStatus, OutgoingBody};
use std::collections::HashMap;
use std::hash::Hash;
use std::io;

pub struct HttpRequestBuilder {
    request: HttpRequest<io::Empty>,
}

impl HttpRequestBuilder {
    pub fn get<S1: AsRef<str>, S2: AsRef<str>>(host: S1, uri: S2) -> Self {
        HttpRequestBuilder::new(host, uri, HttpMethod::Get)
    }

    pub fn put<S1: AsRef<str>, S2: AsRef<str>>(host: S1, uri: S2) -> Self {
        HttpRequestBuilder::new(host, uri, HttpMethod::Put)
    }

    pub fn new<S1: AsRef<str>, S2: AsRef<str>>(host: S1, uri: S2, method: HttpMethod) -> Self {
        let mut request = HttpRequest::new(method, uri.as_ref());
        request.add_header("Host", host.as_ref());
        request.add_header("User-Agent", "http_io");
        request.add_header("Accept", "*/*");
        HttpRequestBuilder { request }
    }

    pub fn send<S: io::Read + io::Write>(self, socket: S) -> Result<OutgoingBody<S>> {
        self.request.serialize(io::BufWriter::new(socket))
    }

    pub fn add_header<S1: AsRef<str>, S2: AsRef<str>>(mut self, key: S1, value: S2) -> Self {
        self.request.add_header(key.as_ref(), value.as_ref());
        self
    }
}

pub trait ToStreamAddr {
    type target;
    fn to_stream_addr(s: Self) -> Result<Self::target>;
}

pub trait StreamConnector {
    type stream: io::Read + io::Write;
    type stream_addr: Hash + Eq + Clone;
    fn connect(a: Self::stream_addr) -> Result<Self::stream>;
}

impl<T> ToStreamAddr for T
where
    T: AsRef<str>,
{
    type target = std::net::SocketAddr;
    fn to_stream_addr(t: T) -> Result<Self::target> {
        let err = || {
            std::io::Error::new(
                std::io::ErrorKind::AddrNotAvailable,
                format!("Failed to lookup {}", t.as_ref()),
            )
        };
        Ok(std::net::ToSocketAddrs::to_socket_addrs(&(t.as_ref(), 80))
            .map_err(|_| err())?
            .next()
            .ok_or(err())?)
    }
}

impl StreamConnector for std::net::TcpStream {
    type stream = std::net::TcpStream;
    type stream_addr = std::net::SocketAddr;
    fn connect(a: Self::stream_addr) -> Result<Self::stream> {
        Ok(std::net::TcpStream::connect(a)?)
    }
}

pub struct HttpClient<S: StreamConnector> {
    streams: HashMap<S::stream_addr, S::stream>,
}

impl<S: StreamConnector> HttpClient<S> {
    pub fn new() -> Self {
        HttpClient {
            streams: HashMap::new(),
        }
    }

    fn get_socket<A: ToStreamAddr<target = S::stream_addr>>(
        &mut self,
        host: A,
    ) -> Result<&mut S::stream> {
        let stream_addr = ToStreamAddr::to_stream_addr(host)?;
        if !self.streams.contains_key(&stream_addr) {
            let stream = S::connect(stream_addr.clone())?;
            self.streams.insert(stream_addr.clone(), stream);
        }
        Ok(self.streams.get_mut(&stream_addr).unwrap())
    }

    pub fn get<S1: AsRef<str> + ToStreamAddr<target = S::stream_addr>, S2: AsRef<str>>(
        &mut self,
        host: S1,
        uri: S2,
    ) -> Result<OutgoingBody<&mut S::stream>> {
        Ok(HttpRequestBuilder::get(host.as_ref(), uri).send(self.get_socket(host)?)?)
    }

    pub fn put<S1: AsRef<str> + ToStreamAddr<target = S::stream_addr>, S2: AsRef<str>>(
        &mut self,
        host: S1,
        uri: S2,
    ) -> Result<OutgoingBody<&mut S::stream>> {
        Ok(HttpRequestBuilder::put(host.as_ref(), uri).send(self.get_socket(host)?)?)
    }
}

pub fn get<S1: AsRef<str>, S2: AsRef<str>>(
    host: S1,
    uri: S2,
) -> Result<HttpBody<std::net::TcpStream>> {
    let s = std::net::TcpStream::connect((host.as_ref(), 80))?;
    let response = HttpRequestBuilder::get(host, uri).send(s)?.finish()?;

    if response.status != HttpStatus::OK {
        return Err(Error::UnexpectedStatus(response.status));
    }

    Ok(response.body)
}

pub fn put<S1: AsRef<str>, S2: AsRef<str>, R: io::Read>(
    host: S1,
    uri: S2,
    mut body: R,
) -> Result<HttpBody<std::net::TcpStream>> {
    let s = std::net::TcpStream::connect((host.as_ref(), 80))?;
    let mut request = HttpRequestBuilder::get(host, uri).send(s)?;

    io::copy(&mut body, &mut request)?;

    let response = request.finish()?;

    if response.status != HttpStatus::OK {
        return Err(Error::UnexpectedStatus(response.status));
    }

    Ok(response.body)
}
