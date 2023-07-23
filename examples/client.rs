use clap::Parser;
use http_io::error::{Error, Result};
use http_io::protocol::HttpMethod;
use http_io::url::Url;
use std::fs::File;
use std::io;

#[derive(Parser)]
struct Options {
    #[clap(long = "method", default_value = "GET")]
    method: HttpMethod,
    #[clap(long = "data", default_value = "")]
    data: String,
    url: Url,
}

fn main() -> Result<()> {
    let opts = Options::parse();
    let mut body = match opts.method {
        HttpMethod::Get => http_io::client::get(opts.url)?,
        HttpMethod::Put => {
            if let Ok(file) = File::open(&opts.data) {
                http_io::client::put(opts.url, file)?
            } else {
                http_io::client::put(opts.url, opts.data.as_bytes())?
            }
        }
        m => return Err(Error::UnexpectedMethod(m)),
    };
    io::copy(&mut body, &mut io::stdout())?;
    Ok(())
}
