use http_io::client::HttpClient;
use http_io::error::Result;
use http_io::url::Url;
use std::io;

fn main() -> Result<()> {
    let args = std::env::args();
    let url: Url = args
        .skip(1)
        .next()
        .unwrap_or("http://www.google.com".into())
        .parse()?;

    let mut client = HttpClient::<std::net::TcpStream>::new();
    for path in &["/", "/favicon.ico", "/robots.txt"] {
        let mut url = url.clone();
        url.path = path.parse()?;
        io::copy(&mut client.get(url)?.finish()?.body, &mut io::stdout())?;
    }

    Ok(())
}
