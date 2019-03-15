use std::convert;
use std::io::{self, Read, Write};
use std::net;
use std::path::PathBuf;

use http_io::client::HttpClient;
use http_io::error::Result;
use http_io::protocol::{HttpBody, HttpResponse, HttpStatus};
use http_io::server::{HttpRequestHandler, HttpServer};

fn client_main(mut args: std::env::Args) -> Result<()> {
    let host = args.next().unwrap_or("www.google.com".into());

    let s = net::TcpStream::connect((host.as_ref(), 80))?;
    let h = HttpClient::new(s);
    let mut response = h.get(host, "/")?;

    let mut body = Vec::new();
    response.body.read_to_end(&mut body)?;

    println!("{:#?}", response.headers);
    io::stdout().write(&body)?;
    Ok(())
}

fn simple_client_main(mut args: std::env::Args) -> Result<()> {
    let host = args.next().unwrap_or("www.google.com".into());
    let mut body = Vec::new();
    http_io::client::get(host, "/")?.read_to_end(&mut body)?;
    io::stdout().write(&body)?;
    Ok(())
}

struct FileHandler {
    file_root: PathBuf,
}

impl FileHandler {
    fn new<P: Into<PathBuf>>(file_root: P) -> Self {
        FileHandler {
            file_root: file_root.into(),
        }
    }
}

impl<I: io::Read> HttpRequestHandler<I> for FileHandler {
    fn get(&self, uri: &str, _stream: HttpBody<&mut I>) -> Result<HttpResponse<Box<dyn io::Read>>> {
        let path = self.file_root.join(uri.trim_start_matches("/"));
        println!("Request for {:?}", path);
        if std::fs::metadata(&path)?.is_dir() {
            let mut page = String::new();
            for entry in std::fs::read_dir(&path)? {
                let entry = entry?;
                if let Some(name) = entry.file_name().to_str() {
                    let link = name.to_owned() + if entry.metadata()?.is_dir() { "/" } else { "" };
                    page += &format!("<a href=\"{}\">{}</a></br>", link, name,);
                }
            }
            Ok(HttpResponse::new(
                HttpStatus::OK,
                Box::new(io::Cursor::new(page)),
            ))
        } else {
            Ok(HttpResponse::new(
                HttpStatus::OK,
                Box::new(std::fs::File::open(path)?),
            ))
        }
    }

    fn put(
        &self,
        uri: &str,
        mut stream: HttpBody<&mut I>,
    ) -> Result<HttpResponse<Box<dyn io::Read>>> {
        let path = self.file_root.join(uri.trim_start_matches("/"));
        println!("Uploading to {:?}", path);
        let mut file = std::fs::File::create(path)?;
        io::copy(&mut stream, &mut file)?;
        Ok(HttpResponse::new(HttpStatus::OK, Box::new(io::empty())))
    }
}

fn server_main(_args: std::env::Args) -> Result<()> {
    let handler = FileHandler::new(std::env::current_dir()?);
    let socket = net::TcpListener::bind("127.0.0.1:8080")?;
    let server = HttpServer::new(socket, handler);
    println!("Server started on port 8080");
    server.serve_forever();
}

fn main() -> Result<()> {
    let mut args = std::env::args();
    args.next();
    let command = args.next();
    match command.as_ref().map(convert::AsRef::as_ref) {
        Some("client") => client_main(args),
        Some("simple_client") => simple_client_main(args),
        Some("server") => server_main(args),
        _ => panic!("Bad arguments"),
    }
}
