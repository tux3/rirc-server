use tokio::net::TcpStream;
use tokio::io::{AsyncRead};
use message::{Message, MessageSink, MessageStream, ReplyCode, make_reply_msg};
use channel::{Channel};
use futures::{Stream, Sink, Future, future};
use server::ServerState;
use std::sync::{Arc, Weak, Mutex, RwLock};
use std::cell::Cell;
use std::net::SocketAddr;
use std::io::{Error, ErrorKind, BufReader};
use std::collections::{HashMap, HashSet};
use std::collections::hash_map::{Entry};

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
                channels: RwLock::new(HashMap::new()),
            },
        }
    }
}

pub struct Client {
    sink: Mutex<Cell<Option<Box<Sink<SinkItem=Message, SinkError=Error> + Send + Sync>>>>,
    pub server_state: Arc<ServerState>,
    pub addr: SocketAddr,
    pub status: ClientStatus,
    pub channels: RwLock<HashMap<String, Weak<RwLock<Channel>>>>,
}

impl Drop for Client {
    fn drop(&mut self) {
        (self.server_state.callbacks.on_client_disconnect)(&self.addr).ok();

        match self.status {
            ClientStatus::Unregistered(_) => (),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => {
                self.broadcast(Message {
                    tags: Vec::new(),
                    source: Some(self.get_extended_prefix().unwrap()),
                    command: "QUIT".to_owned(),
                    params: vec!("Quit".to_owned()),
                }, false).wait().ok();

                self.server_state.users.lock().expect("State users lock")
                    .remove(&nick.to_ascii_uppercase()).expect("Dropped client was registered, but not in users list!");
            },
        };

        self.server_state.clients.lock().expect("State client lock")
            .remove(&self.addr.to_string()).expect("Dropped client was not in client list!");
    }
}

impl Client {
    pub fn get_host(&self) -> String {
        self.addr.ip().to_string()
    }

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

    pub fn get_realname(&self) -> Option<String> {
        match self.status {
            ClientStatus::Unregistered(ref state) => state.realname.clone(),
            ClientStatus::Normal(ref state) => Some(state.realname.clone()),
        }
    }

    pub fn get_extended_prefix(&self) -> Option<String> {
        let nick = self.get_nick()?;
        let username = self.get_username()?;
        Some(nick + "!" + &username + "@" + &self.get_host())
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

    /// Broadcasts a message to all users of all channels this user is in, and optionally to the user itself
    pub fn broadcast(&self, message: Message, include_self: bool) -> Box<Future<Item=(), Error=Error> + Send> {
        let mut users_sent_to = HashSet::new();
        if include_self {
            users_sent_to.insert(self.addr.to_string());
            self.send(message.clone()).wait().ok();
        }

        let channels_guard = self.channels.read().expect("User channels read lock");
        for channel_weak in channels_guard.values() {
            let channel_lock = match channel_weak.upgrade() {
                Some(channel) => channel,
                None => continue,
            };
            let channel_guard = channel_lock.read().expect("Channel read lock");

            let channel_users = channel_guard.users.read().expect("Channel users read lock");
            for (user_addr, weak_user) in channel_users.iter() {
                if !users_sent_to.insert(user_addr.to_string()) {
                    continue
                }

                let user_lock = match weak_user.upgrade() {
                    Some(user) => user,
                    None => continue,
                };
                let user_guard = user_lock.read().expect("User read lock");
                user_guard.send(message.clone()).wait().ok();
            }
        }

        Box::new(future::ok(()))
    }

    /// Sends RPL_ISSUPPORT feature advertisment messages to the client
    pub fn send_issupport(&self) -> Box<Future<Item=(), Error=Error> + Send> {
        let nick = match self.status {
            ClientStatus::Unregistered(_) => panic!("send_issupport called on unregistered client!"),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => nick.clone(),
        };
        let state = &self.server_state;

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
    pub fn send_lusers(&self) -> Box<Future<Item=(), Error=Error> + Send> {
        let nick = match self.status {
            ClientStatus::Unregistered(_) => panic!("send_luser called on unregistered client!"),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => nick.clone(),
        };
        let state = &self.server_state;

        // TODO: Track the number of channels!
        // TODO: Track invisibles, so we can substract them from the visible users count
        let num_channels = state.channels.lock().expect("State channels lock broken").len();
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
    pub fn send_motd(&self) -> Box<Future<Item=(), Error=Error> + Send> {
        let nick = match self.status {
            ClientStatus::Unregistered(_) => panic!("send_motd called on unregistered client!"),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => nick.clone(),
        };

        self.send(make_reply_msg(&self.server_state, &nick, ReplyCode::ErrNoMotd))
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
    pub fn try_finish_registration(&mut self) -> Box<Future<Item=(), Error=Error> + Send> {
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

        let state = self.server_state.clone();
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

        match (state.callbacks.on_client_registering)(self) {
            Ok(true) => (),
            Ok(false) => return self.close_with_error("Rejected by server"),
            Err(e) => return self.close_with_error(&e.to_string()),
        };

        self.send_all(&[
            make_reply_msg(&state, &cur_nick, ReplyCode::RplWelcome),
            make_reply_msg(&state, &cur_nick, ReplyCode::RplYourHost),
            make_reply_msg(&state, &cur_nick, ReplyCode::RplCreated),
            make_reply_msg(&state, &cur_nick, ReplyCode::RplMyInfo),
        ]);
        self.send_issupport();
        self.send_lusers();
        self.send_motd();

        (state.callbacks.on_client_registered)(self).ok();

        Box::new(future::ok(()))
    }

    /// Joins a channel, assuming it doesn't violate any rules
    pub fn join(&mut self, chan_name: &str) -> Box<Future<Item=(), Error=Error> + Send> {
        if !chan_name.starts_with("#") {
            return Box::new(future::err(Error::new(ErrorKind::InvalidInput, "Channels must start with a #")));
        }
        if self.channels.read().unwrap().len() >= self.server_state.settings.chan_limit {
            return Box::new(future::err(Error::new(ErrorKind::Other, "Cannot join, too many channels")));
        }

        let mut channels = self.server_state.channels.lock().expect("Channels lock broken");
        let channel_arc = match channels.entry(chan_name.to_ascii_uppercase()) {
            Entry::Occupied(entry) => entry.get().clone(),
            Entry::Vacant(entry) => {
                entry.insert(Arc::new(RwLock::new(Channel::new(chan_name.to_owned())))).clone()
            },
        };

        {
            let mut client_chans_guard = self.channels.write().expect("Client channels write lock broken");
            match client_chans_guard.entry(chan_name.to_ascii_uppercase()) {
                Entry::Occupied(_) => return Box::new(future::ok(())),
                Entry::Vacant(entry) => {
                    entry.insert(Arc::downgrade(&channel_arc)).clone();
                },
            };
        }

        let weak_self = match self.server_state.clients.lock().expect("Failed to lock clients vector").get(&self.addr.to_string()) {
            Some(weak) => weak.clone(),
            None => return Box::new(future::err(Error::new(ErrorKind::Other, "User completed registration, but is not in the client list!"))),
        };

        let channel_guard = channel_arc.read().expect("Channel read lock broken");
        let chan_join_msgs = channel_guard.get_join_msgs(&self.server_state, &self.get_nick().unwrap());
        let mut chan_users_guard = channel_guard.users.write().expect("Channel users lock broken");
        chan_users_guard.insert(self.addr.to_string(), weak_self);

        let join_msg = Message {
            tags: Vec::new(),
            source: Some(self.get_extended_prefix().expect("JOIN sent by user without a prefix!")),
            command: "JOIN".to_owned(),
            params: vec!(channel_guard.name.to_owned()),
        };

        let addr_string = self.addr.to_string();
        for (chan_user_addr, chan_user_weak) in chan_users_guard.iter() {
            if *chan_user_addr == addr_string {
                continue
            }
            let chan_user = match chan_user_weak.upgrade() {
                Some(user) => user,
                None => continue,
            };
            let chan_user_guard = chan_user.read().expect("Chan user read lock broken");
            chan_user_guard.send(join_msg.clone());
        }
        drop(chan_users_guard);

        self.send(join_msg);
        self.send_all(&chan_join_msgs)
    }

    /// Quits a channel, assuming the channel exists and the user is in it
    pub fn part(&self, channel_name: &str) -> Box<Future<Item=(), Error=Error> + Send> {
        let mut channels_guard = self.channels.write().expect("User channels write lock");
        let channel = match channels_guard.remove(&channel_name.to_ascii_uppercase()).and_then(|weak| weak.upgrade()) {
            Some(channel) => channel,
            None => return self.send(make_reply_msg(&self.server_state, &self.get_nick().unwrap(),
                                                    ReplyCode::ErrNotOnChannel{channel: channel_name.to_owned()})),
        };
        drop(channels_guard);

        let channel_guard = channel.read().expect("Channel read lock");
        let fut = channel_guard.send(Message {
            tags: Vec::new(),
            source: Some(self.get_extended_prefix().expect("part called on a user without a prefix!")),
            command: "PART".to_owned(),
            params: vec!(channel_guard.name.to_owned()),
        }, None);
        drop(channel_guard);

        let channel_guard = channel.write().expect("Channel write lock");
        let mut channel_users = channel_guard.users.write().expect("Channel users write lock");
        channel_users.remove(&self.addr.to_string());

        if channel_users.len() == 0 {
            let mut server_channels = self.server_state.channels.lock().expect("State channels lock");
            server_channels.remove(&channel_guard.name.to_ascii_uppercase());
        }

        fut
    }
}
