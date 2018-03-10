use client::{ClientDuplex, Client};
use std::net::SocketAddr;
use tokio;
use tokio::net::{TcpListener};
use futures::{Future, Stream};
use std::io::{Error};
use message::Message;

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
            Server::handle_client(ClientDuplex::new(socket));

            Ok(())
        }).map_err(|_| ());

        tokio::run(server_fut);
    }

    fn handle_client(client_duplex: ClientDuplex) {
        let client = client_duplex.client;
        println!("New client: {}", client.addr.to_string());

        let fut = client_duplex.stream
            .fold(client, |client, msg| {
            Server::process_message(client, msg)
        });

        tokio::spawn(fut.then(|_| Ok(())));
    }

    fn process_message(client: Client, msg: Message) -> Box<Future<Item=Client, Error=Error>  + Send> {
        client.send(msg)
    }
}
