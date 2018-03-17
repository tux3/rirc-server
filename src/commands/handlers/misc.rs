use client::{Client, ClientStatus};
use server::ServerState;
use message::Message;
use reply_codes::{make_reply_msg, ReplyCode};
use futures::{Future, future};
use std::io::{Error, ErrorKind};
use std::sync::{Arc, RwLock};

pub fn handle_notice(_: Arc<ServerState>, _: Arc<RwLock<Client>>, _: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    // TODO: Actually forward notices to other users and channels
    Box::new(future::ok(()))
}

pub fn handle_version(state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    let client = client.read().expect("Client read lock broken");
    if let Some(target) = msg.params.get(0) {
        if target != &state.settings.server_name {
            return command_error!(state, client, ReplyCode::ErrNoSuchServer{server: target.clone()});
        }
    };

    let nick = client.get_nick().unwrap_or("*".to_owned());
    client.send(make_reply_msg(&state, &nick, ReplyCode::RplVersion {comments: String::new()}));
    client.send_issupport(&state)
}

pub fn handle_lusers(state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    let client = client.read().expect("Client read lock broken");
    if let Some(target) = msg.params.get(0) {
        if target != &state.settings.server_name {
            return command_error!(state, client, ReplyCode::ErrNoSuchServer{server: target.clone()});
        }
    };

    client.send_lusers(&state)
}

pub fn handle_motd(state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    let client = client.read().expect("Client read lock broken");
    if let Some(target) = msg.params.get(0) {
        if target != &state.settings.server_name {
            return command_error!(state, client, ReplyCode::ErrNoSuchServer{server: target.clone()});
        }
    };

    client.send_motd(&state)
}

pub fn handle_privmsg(state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    let client = client.read().expect("Client read lock broken");
    let target = match msg.params.get(0) {
        Some(nick) => nick,
        None => return command_error!(state, client, ReplyCode::ErrNoRecipient{cmd: "PRIVMSG".to_owned()}),
    };
    let msg_text = match msg.params.get(1) {
        Some(msg_text) => msg_text,
        None => return command_error!(state, client, ReplyCode::ErrNoTextToSend),
    };

    if let Some(channel_ref) = state.channels.lock().expect("State channels lock broken").get(&target.to_ascii_uppercase()) {
        let channel_lock = channel_ref.clone();
        let channel_guard = channel_lock.read().expect("Channel lock broken");
        channel_guard.send(Message {
            tags: Vec::new(),
            source: Some(client.get_extended_prefix().expect("PRIVMSG sent by user without a prefix!")),
            command: "PRIVMSG".to_owned(),
            params: vec!(channel_guard.name.to_owned(), msg_text.to_owned()),
        }, Some(client.addr.to_string()))
    } else if target.to_ascii_uppercase() == client.get_nick().expect("PRIVMSG sent by user without a nick!").to_ascii_uppercase() {
        let nick = client.get_nick().unwrap().to_owned();
        client.send(Message {
            tags: Vec::new(),
            source: Some(client.get_extended_prefix().expect("PRIVMSG sent by user without a prefix!")),
            command: "PRIVMSG".to_owned(),
            params: vec!(nick, msg_text.to_owned()),
        })
    } else if let Some(target_user) = state.users.lock().expect("State users lock broken").get(&target.to_ascii_uppercase()) {
        let target_user = match target_user.upgrade() {
            Some(target_user) => target_user,
            None => return command_error!(state, client, ReplyCode::ErrNoSuchNick{nick: target.clone()}),
        };
        let target_user = target_user.read().expect("User read lock broken");
        let nick = target_user.get_nick().unwrap().to_owned();
        target_user.send(Message {
            tags: Vec::new(),
            source: Some(client.get_extended_prefix().expect("PRIVMSG sent by user without a prefix!")),
            command: "PRIVMSG".to_owned(),
            params: vec!(nick, msg_text.to_owned()),
        })
    } else {
        return command_error!(state, client, ReplyCode::ErrNoSuchNick{nick: target.clone()});
    }
}

pub fn handle_quit(_: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    let client = client.read().expect("Client read lock broken");
    let reason = msg.params.get(0).map(|str| str.to_owned()).unwrap_or("Quit".to_owned());
    if let ClientStatus::Unregistered{..} = client.status {
        return Box::new(future::err(Error::new(ErrorKind::Other, reason.clone())));
    }

    client.broadcast(Message {
        tags: Vec::new(),
        source: Some(client.get_extended_prefix().unwrap()),
        command: "QUIT".to_owned(),
        params: vec!(reason.clone()),
    }, true).wait().ok();

    let mut channels = client.channels.write().expect("Client channels write lock broken");
    channels.clear();

    Box::new(future::err(Error::new(ErrorKind::Other, reason.clone())))
}