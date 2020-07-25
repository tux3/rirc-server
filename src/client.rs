use tokio::net::TcpStream;
use tokio::io::BufReader;
use tokio::sync::RwLock;
use futures::{Stream, Sink, SinkExt};
use std::sync::{Arc, Weak};
use std::net::SocketAddr;
use std::io::{Error, ErrorKind};
use std::collections::{HashMap, HashSet};
use std::collections::hash_map::{Entry};
use futures::executor::block_on;
use std::pin::Pin;
use crate::message::{Message, MessageSink, MessageStream, ReplyCode, make_reply_msg};
use crate::channel::{Channel};
use crate::server::ServerState;
use crate::errors::ChannelNotFoundError;
use crate::mode::{UserMode, CHANMODES};

#[cfg(feature = "tls")]
use tokio_rustls::server::TlsStream;

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
    pub stream: Pin<Box<dyn Stream<Item=Result<Message, Error>> + Send>>,
    pub client: Client,
}

impl ClientDuplex {
    pub fn from_tcp_stream(server_state: Arc<ServerState>, socket: TcpStream) -> ClientDuplex {
        let addr = socket.peer_addr().unwrap();
        let (socket_r, socket_w) = socket.into_split();
        let sink = Box::pin(MessageSink::new(socket_w));
        let stream = Box::pin(MessageStream::new(BufReader::new(socket_r)));
        Self::from_sink_and_stream(server_state, addr, stream, sink)
    }

    #[cfg(feature = "tls")]
    pub fn from_tls_stream(server_state: Arc<ServerState>, socket: TlsStream<TcpStream>) -> ClientDuplex {
        let addr = socket.get_ref().0.peer_addr().unwrap();
        let (socket_r, socket_w) = tokio::io::split(socket);
        let sink = Box::pin(MessageSink::new(socket_w));
        let stream = Box::pin(MessageStream::new(BufReader::new(socket_r)));
        Self::from_sink_and_stream(server_state, addr, stream, sink)
    }

    fn from_sink_and_stream(server_state: Arc<ServerState>, addr: SocketAddr,
                            stream: Pin<Box<dyn Stream<Item=Result<Message, Error>> + Send>>,
                            sink: Pin<Box<dyn Sink<Message, Error=Error> + Send + Sync>>) -> ClientDuplex {
        ClientDuplex {
            stream,
            client: Client {
                sink: RwLock::new(sink),
                server_state,
                addr,
                status: ClientStatus::Unregistered(ClientUnregisteredState::new()),
                channels: RwLock::new(HashMap::new()),
                mode: Default::default(),
            },
        }
    }
}

pub struct Client {
    sink: RwLock<Pin<Box<dyn Sink<Message, Error=Error> + Send + Sync>>>,
    pub server_state: Arc<ServerState>,
    pub addr: SocketAddr,
    pub status: ClientStatus,
    pub channels: RwLock<HashMap<String, Weak<RwLock<Channel>>>>,

    pub mode: UserMode,
}

impl Drop for Client {
    fn drop(&mut self) {
        (self.server_state.callbacks.on_client_disconnect)(&self.addr).ok();

        match self.status {
            ClientStatus::Unregistered(_) => (),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => {
                block_on(Box::pin(self.broadcast(Message {
                    tags: Vec::new(),
                    source: Some(self.get_extended_prefix().unwrap()),
                    command: "QUIT".to_owned(),
                    params: vec!("Quit".to_owned()),
                }, false))).ok();

                block_on(self.server_state.users.write())
                    .remove(&nick.to_ascii_uppercase()).expect("Dropped client was registered, but not in users list!");
            },
        };

        block_on(self.server_state.clients.lock())
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
    pub async fn send(&self, msg: Message) -> Result<(), Error> {
        let mut sink = self.sink.write().await;
        sink.send(msg).await?;
        Ok(())
    }

    /// Sends a series of messages in order to the client
    pub async fn send_all(&self, msgs: &[Message]) -> Result<(), Error> {
        for msg in msgs {
            self.send(msg.clone()).await?;
        }
        Ok(())
    }

    /// Broadcasts a message to all users of all channels this user is in, and optionally to the user itself
    pub async fn broadcast(&self, message: Message, include_self: bool) -> Result<(), Error> {
        let mut users_sent_to = HashSet::new();
        if include_self {
            users_sent_to.insert(self.addr.to_string());
            self.send(message.clone()).await?;
        }

        let channels_guard = self.channels.read().await;
        for channel_weak in channels_guard.values() {
            let channel_lock = match channel_weak.upgrade() {
                Some(channel) => channel,
                None => continue,
            };
            let channel_guard = channel_lock.read().await;

            let channel_users = channel_guard.users.read().await;
            for (user_addr, weak_user) in channel_users.iter() {
                if !users_sent_to.insert(user_addr.to_string()) {
                    continue
                }

                let user_lock = match weak_user.upgrade() {
                    Some(user) => user,
                    None => continue,
                };
                let user_guard = user_lock.read().await;
                let _ = user_guard.send(message.clone()).await;
            }
        }

        Ok(())
    }

    /// Sends RPL_ISSUPPORT feature advertisment messages to the client
    pub async fn send_issupport(&self) -> Result<(), Error> {
        let nick = match self.status {
            ClientStatus::Unregistered(_) => panic!("send_issupport called on unregistered client!"),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => nick.clone(),
        };
        let state = self.server_state.clone();

        // For now we don't even need to split it into multiple messages of 12 params each
        let features = vec![
            format!("CASEMAPPING=ascii"),
            format!("CHANLIMIT=#:{}", state.settings.chan_limit),
            format!("CHANMODES={}", CHANMODES),
            format!("CHANNELLEN={}", state.settings.max_channel_length),
            format!("CHANTYPES=#"),
            format!("NETWORK={}", state.settings.network_name),
            format!("NICKLEN={}", state.settings.max_name_length),
            format!("PREFIX"),
            format!("SILENCE"), // No value means we don't support SILENCE
            format!("TOPICLEN={}", state.settings.max_topic_length),
        ];
        self.send(make_reply_msg(&state, &nick, ReplyCode::RplIsSupport {features})).await?;
        Ok(())
    }

    /// Sends RPL_LUSER* replies to the client
    pub async fn send_lusers(&self) -> Result<(), Error> {
        let nick = match self.status {
            ClientStatus::Unregistered(_) => panic!("send_luser called on unregistered client!"),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => nick.clone(),
        };
        let state = self.server_state.clone();

        let num_users;
        let mut num_invisibles = 0;
        {
            let users = state.users.read().await;
            num_users = users.len();
            for weak_user in users.values() {
                if let Some(user) = weak_user.upgrade() {
                    if user.read().await.mode.invisible {
                        num_invisibles += 1;
                    }
                }
            }
        }

        let num_channels = state.channels.lock().await.len();
        let max_users_seen = num_users;
        let num_ops = 0;
        let num_visibles = num_users - num_invisibles;
        let num_unknowns = state.clients.lock().await.len() - num_users;
        self.send_all(&[
            make_reply_msg(&state, &nick, ReplyCode::RplLuserClient {num_visibles, num_invisibles}),
            make_reply_msg(&state, &nick, ReplyCode::RplLuserOp {num_ops}),
            make_reply_msg(&state, &nick, ReplyCode::RplLuserUnknown {num_unknowns}),
            make_reply_msg(&state, &nick, ReplyCode::RplLuserChannels {num_channels}),
            make_reply_msg(&state, &nick, ReplyCode::RplLuserMe {num_users}),
            make_reply_msg(&state, &nick, ReplyCode::RplLocalUsers {num_users, max_users_seen}),
            make_reply_msg(&state, &nick, ReplyCode::RplGlobalUsers {num_users, max_users_seen}),
        ]).await?;

        Ok(())
    }

    /// Sends a MOTD reply to the client
    pub async fn send_motd(&self) -> Result<(), Error> {
        let nick = match self.status {
            ClientStatus::Unregistered(_) => panic!("send_motd called on unregistered client!"),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => nick.clone(),
        };

        self.send(make_reply_msg(&self.server_state, &nick, ReplyCode::ErrNoMotd)).await?;
        Ok(())
    }

    /// Sends an ERROR message and closes down the connection
    pub async fn close_with_error(&self, explanation: &str) -> Result<(), Error> {
        let explanation = explanation.to_owned();
        self.send(Message {
            tags: Vec::new(),
            source: None,
            command: "ERROR".to_owned(),
            params: vec!(format!("Closing Link: {} ({})", &self.addr.ip(), explanation)),
        }).await?;

        Err(Error::new(ErrorKind::Other, explanation))
    }

    /// If the client is ready, try to go through the registration process
    /// Returns true if we still need to finish registration (it is possible to "register" twice)
    pub async fn try_begin_registration(&mut self) -> Result<bool, Error> {
        let cur_nick: String;
        let registered_status = match self.status {
            ClientStatus::Unregistered(ClientUnregisteredState {
                                           nick: Some(ref nick),
                                           username: Some(ref username),
                                           realname: Some(ref realname) }) => {
                cur_nick = nick.clone();
                ClientStatus::Normal(ClientNormalState{nick: nick.clone(), username: username.clone(), realname: realname.clone()})
            },
            _ => return Ok(false),
        };

        let state = self.server_state.clone();
        let weak_self = match state.clients.lock().await.get(&self.addr.to_string()) {
            Some(weak) => weak.clone(),
            None => return Err(Error::new(ErrorKind::Other, "User completed registration, but is not in the client list!")),
        };

        {
            let casemapped_nick = cur_nick.to_ascii_uppercase();
            let mut users_map = state.users.write().await;
            if users_map.contains_key(&casemapped_nick) {
                self.close_with_error("Overridden").await?;
                unreachable!();
            }
            let old_user = users_map.insert(casemapped_nick, weak_self);
            debug_assert!(old_user.is_none());
            self.status = registered_status;
        }

        match (state.callbacks.on_client_registering)(self) {
            Ok(true) => (),
            Ok(false) => self.close_with_error("Rejected by server").await?,
            Err(e) => self.close_with_error(&e.to_string()).await?,
        };

        Ok(true)
    }

    /// Complete the registration process
    pub async fn finish_registration(&self) -> Result<(), Error> {
        let cur_nick = self.get_nick().expect("Must have started registration");
        let state = &self.server_state;
        self.send_all(&[
            make_reply_msg(&state, &cur_nick, ReplyCode::RplWelcome),
            make_reply_msg(&state, &cur_nick, ReplyCode::RplYourHost),
            make_reply_msg(&state, &cur_nick, ReplyCode::RplCreated),
            make_reply_msg(&state, &cur_nick, ReplyCode::RplMyInfo),
        ]).await?;
        self.send_issupport().await?;
        self.send_lusers().await?;
        self.send_motd().await?;

        let _ = (state.callbacks.on_client_registered)(self);

        Ok(())
    }

    /// Joins a channel, assuming it doesn't violate any rules
    pub async fn join(&self, chan_name: &str) -> Result<(), Error> {
        if !chan_name.starts_with('#') {
            return Err(Error::new(ErrorKind::InvalidInput, "Channels must start with a #"));
        }
        if self.channels.read().await.len() >= self.server_state.settings.chan_limit {
            return Err(Error::new(ErrorKind::Other, "Cannot join, too many channels"));
        }

        let channel_arc = {
            let mut channels = self.server_state.channels.lock().await;
            match channels.entry(chan_name.to_ascii_uppercase()) {
                Entry::Occupied(entry) => entry.get().clone(),
                Entry::Vacant(entry) => {
                    entry.insert(Arc::new(RwLock::new(Channel::new(chan_name.to_owned())))).clone()
                },
            }
        };

        {
            let mut client_chans_guard = self.channels.write().await;
            match client_chans_guard.entry(chan_name.to_ascii_uppercase()) {
                Entry::Occupied(_) => return Ok(()),
                Entry::Vacant(entry) => {
                    entry.insert(Arc::downgrade(&channel_arc));
                },
            };
        }

        let weak_self = match self.server_state.clients.lock().await.get(&self.addr.to_string()) {
            Some(weak) => weak.clone(),
            None => return Err(Error::new(ErrorKind::Other, "User completed registration, but is not in the client list!")),
        };

        let channel_guard = channel_arc.read().await;
        let mut chan_users_guard = channel_guard.users.write().await;
        chan_users_guard.insert(self.addr.to_string(), weak_self);
        let chan_join_msgs = channel_guard.get_join_msgs(&self.server_state, &self.get_nick().unwrap()).await;

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
            let chan_user_guard = chan_user.read().await;
            chan_user_guard.send(join_msg.clone()).await?;
        }
        drop(chan_users_guard);

        self.send(join_msg).await?;
        self.send_all(&chan_join_msgs).await
    }

    /// Quits a channel, assuming the channel exists and the user is in it
    pub async fn part(&self, channel_name: &str) -> Result<(), Error> {
        let channel = {
            let mut channels_guard = self.channels.write().await;
            channels_guard.remove(&channel_name.to_ascii_uppercase()).and_then(|weak| weak.upgrade())
        };
        if channel.is_none() {
            return Err(Error::new(ErrorKind::NotFound, ChannelNotFoundError::new(channel_name.to_owned())))
        }
        let channel = channel.unwrap();

        let channel_guard = channel.read().await;
        let result = channel_guard.send(Message {
            tags: Vec::new(),
            source: Some(self.get_extended_prefix().expect("part called on a user without a prefix!")),
            command: "PART".to_owned(),
            params: vec!(channel_guard.name.to_owned()),
        }, None).await;
        drop(channel_guard);

        let channel_guard = channel.read().await;
        let mut channel_users = channel_guard.users.write().await;
        channel_users.remove(&self.addr.to_string());

        if channel_users.len() == 0 {
            let mut server_channels = self.server_state.channels.lock().await;
            server_channels.remove(&channel_guard.name.to_ascii_uppercase());
        }

        result
    }
}
