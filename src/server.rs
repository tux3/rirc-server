use client::{ClientDuplex, Client, ClientStatus};
use channel::{Channel};
use std::net::SocketAddr;
use tokio::{self};
use tokio::net::{TcpListener};
use futures::{Future, Stream, future};
use chrono::{DateTime, Local};
use std::io::{Error};
use std::sync::{Arc, Weak, Mutex, RwLock};
use message::{Message, make_reply_msg, ReplyCode};
use commands::{COMMANDS, is_command_available};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct ServerSettings {
    /// Network address/port to listen on
    pub listen_addr: SocketAddr,
    /// Name the server will use to identify itself
    pub server_name: String,
    /// Advertised network name for this server
    pub network_name: String,
    /// Maximum length of nicknames and usernames
    /// Note that the madatory leading "~" in usernames counts towards this limit
    pub max_name_length: usize,
    /// Maximum length of a channel name
    pub max_channel_length: usize,
    /// Maximum length of a channel topic
    pub max_topic_length: usize,
    /// Maximum number of #channels a client may join
    pub chan_limit: usize,
    /// Whether regular users can create channels
    pub allow_channel_creation: bool,
}

impl Default for ServerSettings {
    fn default() -> Self {
        ServerSettings{
            listen_addr: "0.0.0.0:6667".parse().unwrap(),
            server_name: "rirc-server".to_owned(),
            network_name: "rIRC".to_owned(),
            max_name_length: 16,
            max_channel_length: 50,
            max_topic_length: 390,
            chan_limit: 120,
            allow_channel_creation: true,
        }
    }
}

pub struct ServerState {
    pub settings: ServerSettings,
    pub clients: Mutex<HashMap<String, Weak<RwLock<Client>>>>, // Peer addr -> Client
    pub users: Mutex<HashMap<String, Weak<RwLock<Client>>>>, // Nickname -> Registered Client
    pub channels: Mutex<HashMap<String, Arc<RwLock<Channel>>>>, // Channel name -> Channel
    pub creation_time: DateTime<Local>,
}

impl ServerState {
    pub fn new(settings: ServerSettings) -> Arc<ServerState> {
        Arc::new(ServerState{
            settings,
            creation_time: Local::now(),
            clients: Mutex::new(HashMap::new()),
            users: Mutex::new(HashMap::new()),
            channels: Mutex::new(HashMap::new()),
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
            Server::handle_client(state.clone(), ClientDuplex::new(state, socket));

            Ok(())
        }).map_err(|_| ());

        tokio::run(server_fut);
    }

    fn handle_client(state: Arc<ServerState>, client_duplex: ClientDuplex) {
        let addr = client_duplex.client.addr.clone();
        println!("New client: {}", &addr);
        let client = Arc::new(RwLock::new(client_duplex.client));
        {
            let old_client = state.clients.lock().expect("State client lock")
                                                                .insert(addr.to_string(), Arc::downgrade(&client));
            debug_assert!(old_client.is_none());
        }

        let fut = client_duplex.stream
        .fold(client, move |client, msg| {
            //let state = state_ref.upgrade().expect("Server state dropped while still accepting clients!");
            Server::process_message(state.clone(), client, msg)
        }).then(move |_| {
            println!("Client {} disconnected", &addr);
            Ok(())
        });

        tokio::spawn(fut);
    }

    fn process_message(state: Arc<ServerState>, client_lock: Arc<RwLock<Client>>, msg: Message) -> Box<Future<Item=Arc<RwLock<Client>>, Error=Error> + Send> {
        let fut = if let Some(command) = COMMANDS.get(&msg.command.to_ascii_uppercase() as &str) {
            if is_command_available(&command, &client_lock.read().unwrap()) {
                (command.handler)(state.clone(), client_lock.clone(), msg)
            } else {
                Box::new(future::ok(()))
            }
        } else {
            // We need two blocks to end the client nick's borrow before the send. Thanks, borrowck.
            let client = client_lock.read().unwrap();
            let maybe_nick = match client.status {
                ClientStatus::Normal(ref client_status) => Some(client_status.nick.clone()),
                _ => None,
            };

            if let Some(nick) = maybe_nick {
                client.send(make_reply_msg(&state, &nick, ReplyCode::ErrUnknownCommand{cmd: msg.command.clone()}))
            } else {
                Box::new(future::ok(()))
            }
        };

        Box::new(fut.map(|()| client_lock))
    }
}
