use client::{ClientDuplex, Client};
use std::net::SocketAddr;
use tokio;
use tokio::net::{TcpListener};
use futures::{Future, Stream};
use std::io::{Error};
use std::sync::{Arc};
use message::Message;

#[derive(Clone, Debug)]
pub struct ServerSettings {
    pub listen_addr: SocketAddr,
    pub server_name: String,
}

struct ServerState {
    settings: ServerSettings,
}

impl ServerState {
    pub fn new(settings: ServerSettings) -> Arc<ServerState> {
        Arc::new(ServerState{
            settings,
        })
    }
}

pub struct Server {
    state: Arc<ServerState>,
}

impl Server {
    pub fn new(settings: ServerSettings) -> Server {
        Server {
            state: ServerState::new(settings),
        }
    }

    pub fn start(&mut self) {
        let state_ref = Arc::downgrade(&self.state);
        let listener = TcpListener::bind(&self.state.settings.listen_addr).unwrap();
        
        let server_fut = listener.incoming().for_each(move | socket| {
            let state = state_ref.upgrade().expect("Server state dropped while still accepting clients!");
            Server::handle_client(state, ClientDuplex::new(socket));

            Ok(())
        }).map_err(|_| ());

        tokio::run(server_fut);
    }

    fn handle_client(state: Arc<ServerState>, client_duplex: ClientDuplex) {
        let client = client_duplex.client;
        println!("New client: {}", client.addr.to_string());

        let fut = client_duplex.stream
            .fold(client, move |client, msg| {
                //let state = state_ref.upgrade().expect("Server state dropped while still accepting clients!");
                Server::process_message(state.clone(), client, msg)
            });

        tokio::spawn(fut.then(|_| Ok(())));
    }

    fn process_message(state: Arc<ServerState>, client: Client, msg: Message) -> Box<Future<Item=Client, Error=Error>  + Send> {
        let mut reply = msg.clone();
        reply.source = Some(state.settings.server_name.clone());
        client.send(reply)
    }
}
