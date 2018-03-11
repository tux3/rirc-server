extern crate rirc_server;

use rirc_server::{Server, ServerSettings};

#[test]
fn can_instantiate_server() {
    let _ = Server::new(ServerSettings {
        listen_addr: "0.0.0.0:6667".parse().unwrap(),
        server_name: "test-server".to_owned(),
        ..Default::default()
    });
}
