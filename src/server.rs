use crate::settings::ServerSettings;
use crate::callbacks::ServerCallbacks;
use crate::client::{ClientDuplex, Client, ClientStatus};
use crate::channel::{Channel};
use crate::message::{self, Message, make_reply_msg, ReplyCode};
use crate::commands::{COMMANDS, is_command_available};

use futures::StreamExt;
use chrono::{DateTime, Local};
use std::io::Error;
use std::sync::{Arc, Weak};
use std::collections::HashMap;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, Mutex};

#[cfg(feature = "tls")]
use tokio_rustls::{TlsAcceptor, rustls::ServerConfig};

pub struct ServerState {
    pub settings: ServerSettings,
    pub callbacks: ServerCallbacks,
    pub clients: Mutex<HashMap<String, Weak<RwLock<Client>>>>, // Peer addr -> Client
    pub users: RwLock<HashMap<String, Weak<RwLock<Client>>>>, // Nickname -> Registered Client
    pub channels: Mutex<HashMap<String, Arc<RwLock<Channel>>>>, // Channel name -> Channel
    pub creation_time: DateTime<Local>,
}

impl ServerState {
    pub fn new(settings: ServerSettings, callbacks: ServerCallbacks) -> Arc<ServerState> {
        let msg_breathing_room = 96; // Pretty arbitrary, helps avoid running into MAX_LENGTH.
        assert!(settings.max_name_length < message::MAX_LENGTH - msg_breathing_room);
        assert!(settings.max_channel_length < message::MAX_LENGTH - msg_breathing_room);
        assert!(settings.max_topic_length < message::MAX_LENGTH - msg_breathing_room);
        assert!(!settings.server_name.contains(' '));
        assert!(!settings.network_name.contains(' '));

        Arc::new(ServerState{
            settings,
            callbacks,
            creation_time: Local::now(),
            clients: Mutex::new(HashMap::new()),
            users: RwLock::new(HashMap::new()),
            channels: Mutex::new(HashMap::new()),
        })
    }
}

pub struct Server {
    state: Arc<ServerState>,

    #[cfg(feature = "tls")]
    tls_acceptor: Option<TlsAcceptor>,
    #[cfg(not(feature = "tls"))]
    #[allow(dead_code)]
    tls_acceptor: Option<()>,
}

impl Server {
    pub fn new(settings: ServerSettings, callbacks: ServerCallbacks) -> Server {
        Server {
            state: ServerState::new(settings, callbacks),
            tls_acceptor: None,
        }
    }

    #[cfg(feature = "tls")]
    /// Uses the provided TLS configuration for IRC connections
    pub fn use_tls(&mut self, tls_config: Arc<ServerConfig>) {
        self.tls_acceptor = Some(TlsAcceptor::from(tls_config));
    }

    pub async fn start(&mut self) -> Result<(), Error> {
        let mut listener = TcpListener::bind(&self.state.settings.listen_addr).await?;
        let mut incoming = listener.incoming();

        while let Some(socket) = incoming.next().await {
            let socket = socket?;
            let addr = match socket.peer_addr() {
                Ok(a) => a,
                Err(err) => {
                    println!("Failed to get new client's peer addr: {}", err);
                    continue;
                }
            };
            let client = match self.accept_client(socket).await {
                Ok(c) => c,
                Err(err) => {
                    println!("{}: {}", addr, err);
                    continue;
                }
            };

            tokio::spawn(Server::handle_client(self.state.clone(), client));
        }

        Ok(())
    }

    #[cfg(not(feature = "tls"))]
    async fn accept_client(&self, socket: TcpStream) -> Result<ClientDuplex, Error> {
        Ok(ClientDuplex::from_tcp_stream(self.state.clone(), socket))
    }

    #[cfg(feature = "tls")]
    async fn accept_client(&self, socket: TcpStream) -> Result<ClientDuplex, Error> {
        let client = if self.tls_acceptor.is_some() {
            let acceptor = self.tls_acceptor.clone().unwrap();
            let tls_sock = acceptor.accept(socket).await?;

            ClientDuplex::from_tls_stream(self.state.clone(), tls_sock)
        } else {
            ClientDuplex::from_tcp_stream(self.state.clone(), socket)
        };
        Ok(client)
    }

    async fn handle_client(state: Arc<ServerState>, mut client_duplex: ClientDuplex) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let addr = client_duplex.client.addr;
        println!("New client: {}", &addr);
        let client = Arc::new(RwLock::new(client_duplex.client));
        {
            let old_client = state.clients.lock().await
                .insert(addr.to_string(), Arc::downgrade(&client));
            debug_assert!(old_client.is_none());
        }
        match (state.callbacks.on_client_connect)(&addr) {
            Ok(true) => (),
            Ok(false) => return Ok(()),
            Err(err) => return Err(err),
        };

        while let Some(msg) = client_duplex.stream.next().await {
            let msg = msg?;
            Server::process_message(state.clone(), client.clone(), msg).await?;
        }

        println!("Client {} disconnected", &addr);
        Ok(())
    }

    async fn process_message(state: Arc<ServerState>, client_lock: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error> {
        if let Some(command) = COMMANDS.get(&msg.command.to_ascii_uppercase() as &str) {
            if is_command_available(&command, &*client_lock.read().await) {
                (command.handler)(state.clone(), client_lock.clone(), msg).await?;
            }
        } else {
            // We need two blocks to end the client nick's borrow before the send. Thanks, borrowck.
            let client = client_lock.read().await;
            let maybe_nick = match client.status {
                ClientStatus::Normal(ref client_status) => Some(client_status.nick.clone()),
                _ => None,
            };

            if let Some(nick) = maybe_nick {
                client.send(make_reply_msg(&state, &nick, ReplyCode::ErrUnknownCommand{cmd: msg.command.clone()})).await?;
            }
        };

        Ok(())
    }
}
