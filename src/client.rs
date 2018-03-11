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
use std::sync::Arc;

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
    pub fn new(socket: TcpStream) -> ClientDuplex {
        let addr = socket.peer_addr().unwrap();
        let (socket_r, socket_w) = socket.split();
        let stream = Box::new(MessageStream::new(BufReader::new(socket_r)));
        let sink = Box::new(MessageSink::new(socket_w));
        ClientDuplex {
            stream,
            client: Client {
                sink,
                addr,
                status: ClientStatus::Unregistered(ClientUnregisteredState::new()),
            },
        }
    }
}

pub struct Client {
    sink: Box<Sink<SinkItem=Message, SinkError=Error> + Send>,
    pub addr: SocketAddr,
    pub status: ClientStatus,
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

    /// Sends an arbitray message to the client
    pub fn send(self, msg: Message) -> Box<Future<Item=Client, Error=Error> + Send> {
        let Client{sink, addr, status} = self;
        Box::new(sink.send(msg).and_then(move |sink| {
            Ok(Client{
                sink,
                addr,
                status,
            })
        }))
    }

    /// Sends a series of messages in order to the client
    pub fn send_all(self, msgs: &[Message]) -> Box<Future<Item=Client, Error=Error> + Send> {
        let client = Box::new(future::ok(self));
        msgs.iter().fold(client, move |client, msg| {
            let msg = msg.clone();
            Box::new(client.and_then(|c| c.send(msg)))
        })
    }

    /// Sends RPL_ISSUPPORT feature advertisment messages to the client
    pub fn send_issupport(self, state: &ServerState) -> Box<Future<Item=Client, Error=Error> + Send> {
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
    pub fn send_lusers(self, state: &ServerState) -> Box<Future<Item=Client, Error=Error> + Send> {
        let nick = match self.status {
            ClientStatus::Unregistered(_) => panic!("send_luser called on unregistered client!"),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => nick.clone(),
        };

        // TODO: Track and send real numbers!
        let num_users = 0;
        let num_invisibles = 0;
        let num_ops = 0;
        let num_unknowns = 0;
        let num_channels = 0;
        let num_clients = 0;
        let max_clients_seen = num_clients;
        self.send_all(&[
            make_reply_msg(state, &nick, ReplyCode::RplLuserClient {num_users, num_invisibles}),
            make_reply_msg(state, &nick, ReplyCode::RplLuserOp {num_ops}),
            make_reply_msg(state, &nick, ReplyCode::RplLuserUnknown {num_unknowns}),
            make_reply_msg(state, &nick, ReplyCode::RplLuserChannels {num_channels}),
            make_reply_msg(state, &nick, ReplyCode::RplLuserMe {num_clients}),
            make_reply_msg(state, &nick, ReplyCode::RplLocalUsers {num_clients, max_clients_seen}),
            make_reply_msg(state, &nick, ReplyCode::RplGlobalUsers {num_clients, max_clients_seen}),
        ])
    }

    /// Sends a MOTD reply to the client
    pub fn send_motd(self, state: &ServerState) -> Box<Future<Item=Client, Error=Error> + Send> {
        let nick = match self.status {
            ClientStatus::Unregistered(_) => panic!("send_motd called on unregistered client!"),
            ClientStatus::Normal(ClientNormalState{ref nick, ..}) => nick.clone(),
        };

        self.send(make_reply_msg(state, &nick, ReplyCode::ErrNoMotd))
    }

    /// Sends an ERROR message and closes down the connection
    pub fn close_with_error(self, explanation: &str) -> Box<Future<Item=Client, Error=Error> + Send> {
        let explanation = explanation.to_owned();
        Box::new(self.sink.send(Message{
            tags: Vec::new(),
            source: None,
            command: "ERROR".to_owned(),
            params: vec!(format!("Closing Link: {} ({})", &self.addr.ip(), explanation)),
        }).then(|_| {
            Err(Error::new(ErrorKind::Other, explanation))
        }))
    }

    /// If the client is ready, completes the registration process
    pub fn try_finish_registration(self, state: Arc<ServerState>) -> Box<Future<Item=Client, Error=Error> + Send> {
        let (nick, username, realname) = match self.status {
            ClientStatus::Unregistered(ClientUnregisteredState {
                                           nick: Some(ref nick),
                                           username: Some(ref username),
                                           realname: Some(ref realname) })
            => (nick.clone(), username.clone(), realname.clone()),
            _ => return Box::new(future::ok(self)),
        };

        Box::new(Client {
            sink: self.sink,
            addr: self.addr,
            status: ClientStatus::Normal(ClientNormalState{nick: nick.clone(), username, realname}),
        }.send_all(&[
            make_reply_msg(&state, &nick, ReplyCode::RplWelcome),
            make_reply_msg(&state, &nick, ReplyCode::RplYourHost),
            make_reply_msg(&state, &nick, ReplyCode::RplCreated),
            make_reply_msg(&state, &nick, ReplyCode::RplMyInfo),
        ]).and_then(move |client| {
            client.send_issupport(&state).map(|client| (client, state))
        }).and_then(move |(client, state)| {
            client.send_lusers(&state).map(|client| (client, state))
        }).and_then(move |(client, state)| {
            client.send_motd(&state)
        })
        )
    }
}