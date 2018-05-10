use settings::ServerSettings;
use callbacks::ServerCallbacks;
use client::{ClientDuplex, Client, ClientStatus};
use channel::{Channel};
use tokio::{self};
use tokio::net::{TcpListener};
use futures::{Future, Stream, future};
use chrono::{DateTime, Local};
use std::io::{Error};
use std::sync::{Arc, Weak, Mutex, RwLock};
use message::{self, Message, make_reply_msg, ReplyCode};
use commands::{COMMANDS, is_command_available};
use std::collections::HashMap;

pub struct ServerState {
    pub settings: ServerSettings,
    pub callbacks: ServerCallbacks,
    pub clients: Mutex<HashMap<String, Weak<RwLock<Client>>>>, // Peer addr -> Client
    pub users: Mutex<HashMap<String, Weak<RwLock<Client>>>>, // Nickname -> Registered Client
    pub channels: Mutex<HashMap<String, Arc<RwLock<Channel>>>>, // Channel name -> Channel
    pub creation_time: DateTime<Local>,
}

impl ServerState {
    pub fn new(settings: ServerSettings, callbacks: ServerCallbacks) -> Arc<ServerState> {
        let msg_breathing_room = 96; // Pretty arbitrary, helps avoid running into MAX_LENGTH.
        assert!(settings.max_name_length < message::MAX_LENGTH - msg_breathing_room);
        assert!(settings.max_channel_length < message::MAX_LENGTH - msg_breathing_room);
        assert!(settings.max_topic_length < message::MAX_LENGTH - msg_breathing_room);
        assert!(!settings.server_name.contains(" "));
        assert!(!settings.network_name.contains(" "));

        Arc::new(ServerState{
            settings,
            callbacks,
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
    pub fn new(settings: ServerSettings, callbacks: ServerCallbacks) -> Server {
        Server {
            state: ServerState::new(settings, callbacks),
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
        match (state.callbacks.on_client_connect)(&addr) {
            Ok(true) => (),
            _ => return,
        };

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

    fn process_message(state: Arc<ServerState>, client_lock: Arc<RwLock<Client>>, msg: Message) -> impl Future<Item=Arc<RwLock<Client>>, Error=Error> {
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

        fut.map(|()| client_lock)
    }
}
