use http_io::error::Result;
use http_io::protocol::HttpMethod;
use http_io::url::Url;
use std::io;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Options {
    #[structopt(long = "method", default_value = "GET")]
    method: HttpMethod,
    #[structopt(long = "data", default_value = "")]
    data: String,
    url: Url,
}

fn main() -> Result<()> {
    let opts = Options::from_args();
    let mut body = match opts.method {
        HttpMethod::Get => http_io::client::get(opts.url)?,
        HttpMethod::Put => http_io::client::put(opts.url, opts.data.as_bytes())?,
    };
    io::copy(&mut body, &mut io::stdout())?;
    Ok(())
}
