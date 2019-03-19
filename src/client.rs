use crate::error::{Error, Result};
use crate::protocol::{HttpBody, HttpMethod, HttpRequest, HttpResponse, HttpStatus, OutgoingBody};
use std::io;

pub struct HttpClient<S: io::Read + io::Write> {
    socket: S,
}

impl<S: io::Read + io::Write> HttpClient<S> {
    pub fn new(socket: S) -> HttpClient<S> {
        HttpClient { socket }
    }

    pub fn request<S1: AsRef<str>, S2: AsRef<str>>(
        self,
        host: S1,
        method: HttpMethod,
        uri: S2,
    ) -> Result<OutgoingBody<S>> {
        let socket = io::BufWriter::new(self.socket);
        let mut request = HttpRequest::new(method, uri.as_ref());
        request.add_header("Host", host.as_ref());
        request.add_header("User-Agent", "fuck/bitches");
        request.add_header("Accept", "*/*");
        request.serialize(socket)
    }

    pub fn get<S1: AsRef<str>, S2: AsRef<str>>(self, host: S1, uri: S2) -> Result<HttpResponse<S>> {
        Ok(self.request(host, HttpMethod::Get, uri)?.finish()?)
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
    let c = HttpClient::new(s);
    let response = c.get(host, uri.as_ref())?;

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
    let c = HttpClient::new(s);
    let mut request = c.put(host, uri.as_ref())?;
    io::copy(&mut body, &mut request)?;
    let response = request.finish()?;

    if response.status != HttpStatus::OK {
        return Err(Error::UnexpectedStatus(response.status));
    }

    Ok(response.body)
}
