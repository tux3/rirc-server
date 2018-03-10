
use message_stream::{MessageStream};
use client::Client;
use std::net::SocketAddr;
use tokio;
use tokio::io::{AsyncRead, write_all};
use tokio::net::{TcpListener};
use futures::{Future, Stream};
use std::io::BufReader;

pub struct Server {
    addr: SocketAddr,
}

impl Server {
    pub fn new(addr: SocketAddr) -> Server {
        Server {
            addr,
        }
    }

    pub fn start(&mut self) {
        let listener = TcpListener::bind(&self.addr).unwrap();

        let server_fut = listener.incoming().for_each(move | socket| {
            Server::handle_client(Client::new(socket));

            Ok(())
        }).map_err(|_| ());

        tokio::run(server_fut);
    }

    fn handle_client(client: Client) {
        println!("New client: {}", client.addr.to_string());

        let (socket_r, socket_w) = client.socket.split();
        let msg_stream = MessageStream::new(BufReader::new(socket_r));

        let fut = msg_stream.fold(socket_w,|sock_w, msg| {
            let reply = format!("{} {}\n", msg.command, msg.params.join(" ")).into_bytes();
            write_all(sock_w, reply).map(|(sock_w, _)| sock_w)
        })
        .map_err(|_| ())
        .then(|_| Ok(()));

        tokio::spawn(fut);
    }
}
