extern crate rirc_server;

use rirc_server::Server;

#[test]
fn can_instantiate_server() {
    let addr = "0.0.0.0:6667".parse().unwrap();
    let _ = Server::new(addr);
}
