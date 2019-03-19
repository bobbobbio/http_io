use crate::error::{Error, Result};
use crate::protocol::{HttpBody, HttpMethod, HttpRequest, HttpStatus, OutgoingBody};
use std::io;

pub struct HttpRequestBuilder<S: io::Read + io::Write> {
    request: HttpRequest<io::Empty>,
    socket: S,
}

impl<S: io::Read + io::Write> HttpRequestBuilder<S> {
    pub fn new(socket: S) -> HttpRequestBuilder<S> {
        let mut request = HttpRequest::new(HttpMethod::Get, "/");
        request.add_header("User-Agent", "http_io");
        request.add_header("Accept", "*/*");
        HttpRequestBuilder { request, socket }
    }

    pub fn request<S1: AsRef<str>, S2: AsRef<str>>(
        mut self,
        host: S1,
        method: HttpMethod,
        uri: S2,
    ) -> Result<OutgoingBody<S>> {
        self.request.method = method;
        self.request.uri = uri.as_ref().into();
        self.request.add_header("Host", host.as_ref());
        self.request.serialize(io::BufWriter::new(self.socket))
    }

    pub fn add_header<S1: AsRef<str>, S2: AsRef<str>>(&mut self, key: S1, value: S2) {
        self.request.add_header(key.as_ref(), value.as_ref());
    }

    pub fn get<S1: AsRef<str>, S2: AsRef<str>>(self, host: S1, uri: S2) -> Result<OutgoingBody<S>> {
        Ok(self.request(host, HttpMethod::Get, uri)?)
    }

    pub fn put<S1: AsRef<str>, S2: AsRef<str>>(self, host: S1, uri: S2) -> Result<OutgoingBody<S>> {
        Ok(self.request(host, HttpMethod::Put, uri)?)
    }
}

pub fn get<S1: AsRef<str>, S2: AsRef<str>>(
    host: S1,
    uri: S2,
) -> Result<HttpBody<std::net::TcpStream>> {
    let s = std::net::TcpStream::connect((host.as_ref(), 80))?;
    let c = HttpRequestBuilder::new(s);
    let response = c.get(host, uri.as_ref())?.finish()?;

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
    let c = HttpRequestBuilder::new(s);
    let mut request = c.put(host, uri.as_ref())?;
    io::copy(&mut body, &mut request)?;
    let response = request.finish()?;

    if response.status != HttpStatus::OK {
        return Err(Error::UnexpectedStatus(response.status));
    }

    Ok(response.body)
}
