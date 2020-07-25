use rirc_server::{Server, ServerSettings, ServerCallbacks};
use tokio_rustls::rustls::{Certificate, PrivateKey, ServerConfig, NoClientAuth};
use tokio_rustls::rustls::internal::pemfile::{certs, pkcs8_private_keys};
use std::path::{PathBuf, Path};
use std::io::{BufReader, Error, ErrorKind, Result};
use std::fs::File;
use structopt::StructOpt;

#[derive(StructOpt)]
struct Options {
    /// Your fullchain.pem certificate chain
    #[structopt(short="c", long="cert", parse(from_os_str))]
    cert: PathBuf,

    /// Your privkey.pem key
    #[structopt(short="k", long="key", parse(from_os_str))]
    key: PathBuf,
}

fn load_certs(path: &Path) -> Result<Vec<Certificate>> {
    certs(&mut BufReader::new(File::open(path)?))
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "invalid cert"))
}

fn load_keys(path: &Path) -> Result<Vec<PrivateKey>> {
    pkcs8_private_keys(&mut BufReader::new(File::open(path)?))
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "invalid key"))
}

#[tokio::main]
async fn main() -> Result<()> {
    // This TLS example code happily lifted from tokio-rustls/examples/server/src/main.rs
    let options = Options::from_args();
    let certs = load_certs(&options.cert)?;
    let mut keys = load_keys(&options.key)?;

    // NOTE: rustls does NOT like starting a server on an IP, without DNS
    //       If you get CorruptMessagePayload(Handshake) errors on 127.0.0.1, this is why
    //       See https://github.com/briansmith/webpki/issues/54
    let mut tls_config = ServerConfig::new(NoClientAuth::new());
    tls_config.set_single_cert(certs, keys.remove(0)).expect("Failed to set server certificate");

    let mut server = Server::new(ServerSettings {
        listen_addr: "0.0.0.0:6697".parse().unwrap(),
        server_name: "example-tls-server".to_owned(),
        ..Default::default()
    }, ServerCallbacks::default());
    server.use_tls(tls_config.into());

    server.start().await
}
