use std::io;
use std::net;
use std::path::PathBuf;

use http_io::error::Result;
use http_io::protocol::{HttpBody, HttpResponse, HttpStatus};
use http_io::server::{HttpRequestHandler, HttpServer};

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
    fn get(
        &mut self,
        uri: String,
        _stream: HttpBody<&mut I>,
    ) -> Result<HttpResponse<Box<dyn io::Read>>> {
        let path = self.file_root.join(uri.trim_start_matches("/"));
        println!("Request for {:?}", path);
        if std::fs::metadata(&path)?.is_dir() {
            let mut file_list = String::new();
            for entry in std::fs::read_dir(&path)? {
                let entry = entry?;
                if let Some(name) = entry.file_name().to_str() {
                    let link = name.to_owned() + if entry.metadata()?.is_dir() { "/" } else { "" };
                    file_list += &format!("<li><a href=\"{}\">{}</a></br>", link, name,);
                }
            }
            let page = format!(
                r#"
                <html>
                <title>Directory listing for {0}</title>
                <h2>Directory listing for {0}</h2>
                <body>
                <hr>
                <ul>
                {1}
                </ul>
                <hr>
                </body>
                </html>
            "#,
                uri, &file_list
            );
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
        &mut self,
        uri: String,
        mut stream: HttpBody<&mut I>,
    ) -> Result<HttpResponse<Box<dyn io::Read>>> {
        let path = self.file_root.join(uri.trim_start_matches("/"));
        println!("Uploading to {:?}", path);
        let mut file = std::fs::File::create(path)?;
        io::copy(&mut stream, &mut file)?;
        Ok(HttpResponse::new(HttpStatus::OK, Box::new(io::empty())))
    }
}

fn main() -> Result<()> {
    let handler = FileHandler::new(std::env::current_dir()?);
    let socket = net::TcpListener::bind("127.0.0.1:8080")?;
    let mut server = HttpServer::new(socket, handler);
    println!("Server started on port 8080");
    server.serve_forever();
}
