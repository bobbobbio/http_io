use crate::error::{Error, Result};
use crate::protocol::{HttpBody, HttpMethod, HttpRequest, HttpStatus, OutgoingBody};
use crate::url::Url;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt::Display;
use std::hash::Hash;
use std::io;

pub struct HttpRequestBuilder {
    request: HttpRequest<io::Empty>,
}

impl HttpRequestBuilder {
    pub fn get<U: TryInto<Url>>(url: U) -> Result<Self>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        HttpRequestBuilder::new(url, HttpMethod::Get)
    }

    pub fn put<U: TryInto<Url>>(url: U) -> Result<Self>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        HttpRequestBuilder::new(url, HttpMethod::Put)
    }

    pub fn new<U: TryInto<Url>>(url: U, method: HttpMethod) -> Result<Self>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        let url = url
            .try_into()
            .map_err(|e| Error::ParseError(e.to_string()))?;
        let mut request = HttpRequest::new(method, url.path());
        request.add_header("Host", url.authority.as_ref());
        request.add_header("User-Agent", "http_io");
        request.add_header("Accept", "*/*");
        Ok(HttpRequestBuilder { request })
    }

    pub fn send<S: io::Read + io::Write>(self, socket: S) -> Result<OutgoingBody<S>> {
        self.request.serialize(io::BufWriter::new(socket))
    }

    pub fn add_header<S1: AsRef<str>, S2: AsRef<str>>(mut self, key: S1, value: S2) -> Self {
        self.request.add_header(key.as_ref(), value.as_ref());
        self
    }
}

pub trait StreamConnector {
    type stream: io::Read + io::Write;
    type stream_addr: Hash + Eq + Clone;
    fn connect(a: Self::stream_addr) -> Result<Self::stream>;
    fn to_stream_addr(url: Url) -> Result<Self::stream_addr>;
}

impl StreamConnector for std::net::TcpStream {
    type stream = std::net::TcpStream;
    type stream_addr = std::net::SocketAddr;

    fn connect(a: Self::stream_addr) -> Result<Self::stream> {
        Ok(std::net::TcpStream::connect(a)?)
    }

    fn to_stream_addr(url: Url) -> Result<Self::stream_addr> {
        let err = || {
            std::io::Error::new(
                std::io::ErrorKind::AddrNotAvailable,
                format!("Failed to lookup {}", &url.authority),
            )
        };
        Ok(std::net::ToSocketAddrs::to_socket_addrs(&(
            url.authority.as_ref(),
            url.port.unwrap_or(80),
        ))
        .map_err(|_| err())?
        .next()
        .ok_or(err())?)
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

    fn get_socket(&mut self, url: Url) -> Result<&mut S::stream> {
        let stream_addr = S::to_stream_addr(url)?;
        if !self.streams.contains_key(&stream_addr) {
            let stream = S::connect(stream_addr.clone())?;
            self.streams.insert(stream_addr.clone(), stream);
        }
        Ok(self.streams.get_mut(&stream_addr).unwrap())
    }

    pub fn get<U: TryInto<Url>>(&mut self, url: U) -> Result<OutgoingBody<&mut S::stream>>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        let url = url
            .try_into()
            .map_err(|e| Error::ParseError(e.to_string()))?;
        Ok(HttpRequestBuilder::get(url.clone())?.send(self.get_socket(url)?)?)
    }

    pub fn put<U: TryInto<Url>>(&mut self, url: U) -> Result<OutgoingBody<&mut S::stream>>
    where
        <U as TryInto<Url>>::Error: Display,
    {
        let url = url
            .try_into()
            .map_err(|e| Error::ParseError(e.to_string()))?;
        Ok(HttpRequestBuilder::put(url.clone())?.send(self.get_socket(url)?)?)
    }
}

pub fn get<U: TryInto<Url>>(url: U) -> Result<HttpBody<std::net::TcpStream>>
where
    <U as TryInto<Url>>::Error: Display,
{
    let url = url
        .try_into()
        .map_err(|e| Error::ParseError(e.to_string()))?;
    let s = std::net::TcpStream::connect((url.authority.as_ref(), url.port.unwrap_or(80)))?;
    let response = HttpRequestBuilder::get(url)?.send(s)?.finish()?;

    if response.status != HttpStatus::OK {
        return Err(Error::UnexpectedStatus(response.status));
    }

    Ok(response.body)
}

pub fn put<U: TryInto<Url>, R: io::Read>(
    url: U,
    mut body: R,
) -> Result<HttpBody<std::net::TcpStream>>
where
    <U as TryInto<Url>>::Error: Display,
{
    let url = url
        .try_into()
        .map_err(|e| Error::ParseError(e.to_string()))?;
    let s = std::net::TcpStream::connect((url.authority.as_ref(), url.port.unwrap_or(80)))?;
    let mut request = HttpRequestBuilder::get(url)?.send(s)?;

    io::copy(&mut body, &mut request)?;

    let response = request.finish()?;

    if response.status != HttpStatus::OK {
        return Err(Error::UnexpectedStatus(response.status));
    }

    Ok(response.body)
}
