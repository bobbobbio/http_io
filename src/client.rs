use crate::error::{Error, Result};
use crate::protocol::{HttpBody, HttpMethod, HttpRequest, HttpStatus, OutgoingBody};
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

    let mut request = HttpRequestBuilder::put(host, uri).send(s)?;
    io::copy(&mut body, &mut request)?;

    let response = request.finish()?;

    if response.status != HttpStatus::OK {
        return Err(Error::UnexpectedStatus(response.status));
    }

    Ok(response.body)
}
