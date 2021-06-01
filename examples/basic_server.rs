use rirc_server::{Server, ServerCallbacks, ServerSettings};

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    let mut server = Server::new(
        ServerSettings {
            listen_addr: "0.0.0.0:6667".parse().unwrap(),
            server_name: "example-server".to_owned(),
            ..Default::default()
        },
        ServerCallbacks::default(),
    );

    server.start().await
}
