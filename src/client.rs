use std::net::SocketAddr;
use std::io::{Error, ErrorKind, BufReader};
use tokio::net::TcpStream;
use tokio::io::{AsyncRead};
use message::Message;
use message_sink::MessageSink;
use message_stream::MessageStream;
use futures::{Stream, Sink, Future, future};
use server::ServerState;
use reply_codes::{ReplyCode, make_reply_msg};
use std::sync::{Arc, Mutex};
use std::cell::Cell;

pub struct ClientUnregisteredState {
    pub nick: Option<String>,
    pub username: Option<String>,
    pub realname: Option<String>,
}

pub struct ClientNormalState {
    pub nick: String,
    pub username: String,
    pub realname: String,
}

impl ClientUnregisteredState {
    fn new() -> ClientUnregisteredState{
        ClientUnregisteredState{
            nick: None,
            username: None,
            realname: None,
        }
    }
}

pub enum ClientStatus {
    /// State immediately after connecting, before having set Nick and User
    Unregistered(ClientUnregisteredState),
    /// Normal user that completed registration
    Normal(ClientNormalState),
}

pub struct ClientDuplex {
    pub stream: Box<Stream<Item=Message, Error=Error> + Send>,
    pub client: Client,
}

impl ClientDuplex {
    pub fn new(server_state: Arc<ServerState>, socket: TcpStream) -> ClientDuplex {
        let addr = socket.peer_addr().unwrap();
        let (socket_r, socket_w) = socket.split();
        let stream = Box::new(MessageStream::new(BufReader::new(socket_r)));
        ClientDuplex {
            stream,
            client: Client {
                sink: Mutex::new(Cell::new(Some(Box::new(MessageSink::new(socket_w))))),
                server_state,
                addr,
                status: ClientStatus::Unregistered(ClientUnregisteredState::new()),
            },
        }
    }
}

pub struct Client {
    sink: Mutex<Cell<Option<Box<Sink<SinkItem=Message, SinkError=Error> + Send + Sync>>>>,
    server_state: Arc<ServerState>,
    pub addr: SocketAddr,
    pub status: ClientStatus,
}

impl Drop for Client {
    fn drop(&mut self) {
        self.server_state.clients.lock().expect("State client lock")
            .remove(&self.addr.to_string()).expect("Dropped client was not in client list!");
        match self.status {
            ClientStatus::Unregistered(_) => (),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => {
                self.server_state.users.lock().expect("State users lock")
                    .remove(&nick.to_ascii_uppercase()).expect("Dropped client was registered, but not in users list!");
            },
        };
    }
}

impl Client {
    pub fn get_nick(&self) -> Option<String> {
        match self.status {
            ClientStatus::Unregistered(ref state) => state.nick.clone(),
            ClientStatus::Normal(ref state) => Some(state.nick.clone()),
        }
    }

    pub fn get_username(&self) -> Option<String> {
        match self.status {
            ClientStatus::Unregistered(ref state) => state.username.clone(),
            ClientStatus::Normal(ref state) => Some(state.username.clone()),
        }
    }

    pub fn get_extended_prefix(&self) -> Option<String> {
        let nick = self.get_nick()?;
        let username = self.get_username()?;
        Some(nick + "!~" + &username + "@" + &self.addr.ip().to_string())
    }

    /// Sends an arbitrary message to the client
    pub fn send(&self, msg: Message) -> Box<Future<Item=(), Error=Error> + Send> {
        let sink_guard = self.sink.lock().unwrap();
        let sink = sink_guard.take().unwrap();
        sink_guard.set(match sink.send(msg).wait() {
            Ok(sink) => Some(sink),
            Err(e) => return Box::new(future::err(e)),
        });
        Box::new(future::ok(()))
    }

    /// Sends a series of messages in order to the client
    pub fn send_all(&self, msgs: &[Message]) -> Box<Future<Item=(), Error=Error> + Send> {
        let client: &Self = self;

        msgs.iter().fold(Box::new(future::ok(())), move |fut, msg| {
            Box::new(fut.join(client.send(msg.clone())).map(|_| ()))
        })
    }

    /// Sends RPL_ISSUPPORT feature advertisment messages to the client
    pub fn send_issupport(&self, state: &ServerState) -> Box<Future<Item=(), Error=Error> + Send> {
        let nick = match self.status {
            ClientStatus::Unregistered(_) => panic!("send_issupport called on unregistered client!"),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => nick.clone(),
        };

        // For now we don't even need to split it into multiple messages of 12 params each
        let features = vec![
            format!("CASEMAPPING=ascii"),
            format!("CHANLIMIT=#:{}", state.settings.chan_limit),
            format!("CHANMODES=,,,"), // TODO: Actually support the most basic modes like +i
            format!("CHANNELLEN={}", state.settings.max_channel_length),
            format!("CHANTYPES=#"),
            format!("NETWORK={}", state.settings.network_name),
            format!("NICKLEN={}", state.settings.max_name_length),
            format!("PREFIX"),
            format!("SILENCE"), // No value means we don't support SILENCE
            format!("TOPICLEN={}", state.settings.max_topic_length),
        ];
        self.send(make_reply_msg(state, &nick, ReplyCode::RplIsSupport {features}))
    }

    /// Sends RPL_LUSER* replies to the client
    pub fn send_lusers(&self, state: &ServerState) -> Box<Future<Item=(), Error=Error> + Send> {
        let nick = match self.status {
            ClientStatus::Unregistered(_) => panic!("send_luser called on unregistered client!"),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => nick.clone(),
        };

        // TODO: Track the number of channels!
        // TODO: Track invisibles, so we can substract them from the visible users count
        let num_channels = 0;
        let num_users = state.users.lock().expect("State users lock broken").len();
        let max_users_seen = num_users;
        let num_ops = 0;
        let num_invisibles = 0;
        let num_visibles = num_users - num_invisibles;
        let num_unknowns = state.clients.lock().expect("State clients lock broken").len() - num_users;
        self.send_all(&[
            make_reply_msg(state, &nick, ReplyCode::RplLuserClient {num_visibles, num_invisibles}),
            make_reply_msg(state, &nick, ReplyCode::RplLuserOp {num_ops}),
            make_reply_msg(state, &nick, ReplyCode::RplLuserUnknown {num_unknowns}),
            make_reply_msg(state, &nick, ReplyCode::RplLuserChannels {num_channels}),
            make_reply_msg(state, &nick, ReplyCode::RplLuserMe {num_users}),
            make_reply_msg(state, &nick, ReplyCode::RplLocalUsers {num_users, max_users_seen}),
            make_reply_msg(state, &nick, ReplyCode::RplGlobalUsers {num_users, max_users_seen}),
        ])
    }

    /// Sends a MOTD reply to the client
    pub fn send_motd(&self, state: &ServerState) -> Box<Future<Item=(), Error=Error> + Send> {
        let nick = match self.status {
            ClientStatus::Unregistered(_) => panic!("send_motd called on unregistered client!"),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => nick.clone(),
        };

        self.send(make_reply_msg(state, &nick, ReplyCode::ErrNoMotd))
    }

    /// Sends an ERROR message and closes down the connection
    pub fn close_with_error(&mut self, explanation: &str) -> Box<Future<Item=(), Error=Error> + Send> {
        let explanation = explanation.to_owned();
        Box::new(self.send(Message{
            tags: Vec::new(),
            source: None,
            command: "ERROR".to_owned(),
            params: vec!(format!("Closing Link: {} ({})", &self.addr.ip(), explanation)),
        }).then(|_| {
            Err(Error::new(ErrorKind::Other, explanation))
        }))
    }

    /// If the client is ready, completes the registration process
    pub fn try_finish_registration(&mut self, state: Arc<ServerState>) -> Box<Future<Item=(), Error=Error> + Send> {
        let cur_nick: String;
        let registered_status = match self.status {
            ClientStatus::Unregistered(ClientUnregisteredState {
                                           nick: Some(ref nick),
                                           username: Some(ref username),
                                           realname: Some(ref realname) }) => {
                cur_nick = nick.clone();
                ClientStatus::Normal(ClientNormalState{nick: nick.clone(), username: username.clone(), realname: realname.clone()})
            },
            _ => return Box::new(future::ok(())),
        };

        let weak_self = match state.clients.lock().expect("Failed to lock clients vector").get(&self.addr.to_string()) {
            Some(weak) => weak.clone(),
            None => return Box::new(future::err(Error::new(ErrorKind::Other, "User completed registration, but is not in the client list!"))),
        };

        {
            let casemapped_nick = cur_nick.to_ascii_uppercase();
            let mut users_map = state.users.lock().expect("Failed to lock users vector");
            if users_map.contains_key(&casemapped_nick) {
                return self.close_with_error("Overridden");
            }
            let old_user = users_map.insert(casemapped_nick, weak_self);
            debug_assert!(old_user.is_none());
            self.status = registered_status;
        }

        self.send_all(&[
            make_reply_msg(&state, &cur_nick, ReplyCode::RplWelcome),
            make_reply_msg(&state, &cur_nick, ReplyCode::RplYourHost),
            make_reply_msg(&state, &cur_nick, ReplyCode::RplCreated),
            make_reply_msg(&state, &cur_nick, ReplyCode::RplMyInfo),
        ]);
        self.send_issupport(&state);
        self.send_lusers(&state);
        self.send_motd(&state)
    }
}