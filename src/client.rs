use tokio::net::TcpStream;
use std::net::SocketAddr;

pub struct Client {
    pub socket: TcpStream,
    pub addr: SocketAddr,
}

impl Client {
    pub fn new(socket: TcpStream) -> Client {
        let addr = socket.peer_addr().unwrap();
        Client {
            socket,
            addr,
        }
    }
}