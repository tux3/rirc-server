use crate::client::{Client, ClientStatus};
use crate::server::ServerState;
use crate::message::{Message, make_reply_msg, ReplyCode};
use crate::commands::command_error;
use std::io::{Error, ErrorKind};
use std::sync::Arc;
use tokio::sync::RwLock;

pub async fn handle_ping(state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error>{
    let client = client.read().await;

    let mut reply_params = msg.params.clone();
    reply_params.insert(0, state.settings.server_name.clone());

    client.send(Message {
        tags: Vec::new(),
        source: Some(state.settings.server_name.clone()),
        command: "PONG".to_owned(),
        params: reply_params,
    }).await
}

pub async fn handle_version(state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error> {
    let client = client.read().await;
    if let Some(target) = msg.params.get(0) {
        if target != &state.settings.server_name {
            return command_error(&state, &client, ReplyCode::ErrNoSuchServer{server: target.clone()}).await;
        }
    };

    let nick = client.get_nick().unwrap_or_else(|| "*".to_owned());
    client.send(make_reply_msg(&state, &nick, ReplyCode::RplVersion {comments: String::new()})).await?;
    client.send_issupport().await?;
    Ok(())
}

pub async fn handle_lusers(state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error> {
    let client = client.read().await;
    if let Some(target) = msg.params.get(0) {
        if target != &state.settings.server_name {
            return command_error(&state, &client, ReplyCode::ErrNoSuchServer{server: target.clone()}).await;
        }
    };

    client.send_lusers().await
}

pub async fn handle_motd(state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error> {
    let client = client.read().await;
    if let Some(target) = msg.params.get(0) {
        if target != &state.settings.server_name {
            return command_error(&state, &client, ReplyCode::ErrNoSuchServer{server: target.clone()}).await;
        }
    };

    client.send_motd().await
}


pub async fn handle_notice(state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error> {
    handle_notice_or_privmsg(state, client, msg, true).await
}

pub async fn handle_privmsg(state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error> {
    handle_notice_or_privmsg(state, client, msg, false).await
}

pub async fn handle_notice_or_privmsg(state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message, is_notice: bool) -> Result<(), Error> {
    let client = client.read().await;
    let cmd_name = if is_notice { "NOTICE".to_owned() } else { "PRIVMSG".to_owned() };
    let target = match msg.params.get(0) {
        Some(nick) => nick,
        None => return if is_notice {
            Ok(())
        } else {
            command_error(&state, &client, ReplyCode::ErrNoRecipient{cmd: cmd_name.clone()}).await
        },
    };
    let msg_text = match msg.params.get(1) {
        Some(msg_text) => msg_text,
        None => return if is_notice {
            Ok(())
        } else {
            command_error(&state, &client, ReplyCode::ErrNoTextToSend).await
        },
    };

    if let Some(channel_ref) = state.channels.lock().await.get(&target.to_ascii_uppercase()) {
        let channel_lock = channel_ref.clone();
        let channel_guard = channel_lock.read().await;
        match (state.callbacks.on_client_channel_message)(&client, &channel_guard, &msg) {
            Ok(true) => (),
            Ok(false) => return Ok(()),
            Err(e) => return if is_notice {
                Ok(())
            } else {
                command_error(&state, &client, ReplyCode::ErrCannotSendToChan { channel: target.clone(), reason: e.to_string() }).await
            },
        }
        channel_guard.send(Message {
            tags: Vec::new(),
            source: Some(client.get_extended_prefix().expect("Message sent by user without a prefix!")),
            command: cmd_name.clone(),
            params: vec!(channel_guard.name.to_owned(), msg_text.to_owned()),
        }, Some(client.addr.to_string())).await
    } else if target.to_ascii_uppercase() == client.get_nick().expect("Message sent by user without a nick!").to_ascii_uppercase() {
        let nick = client.get_nick().unwrap();
        let prefix = Some(client.get_extended_prefix().expect("Message sent by user without a prefix!"));
        if is_notice {
            Ok(())
        } else {
            client.send(Message {
                tags: Vec::new(),
                source: prefix,
                command: cmd_name.clone(),
                params: vec!(nick, msg_text.to_owned()),
            }).await
        }
    } else if let Some(target_user) = state.users.read().await.get(&target.to_ascii_uppercase()) {
        let target_user = match target_user.upgrade() {
            Some(target_user) => target_user,
            None => return if is_notice {
                Ok(())
            } else {
                command_error(&state, &client, ReplyCode::ErrNoSuchNick{nick: target.clone()}).await
            },
        };
        let target_user = target_user.read().await;
        let nick = target_user.get_nick().unwrap();
        let prefix = Some(client.get_extended_prefix().expect("Message sent by user without a prefix!"));
        target_user.send(Message {
            tags: Vec::new(),
            source: prefix,
            command: cmd_name.clone(),
            params: vec!(nick, msg_text.to_owned()),
        }).await
    } else {
        if is_notice {
            Ok(())
        } else {
            command_error(&state, &client, ReplyCode::ErrNoSuchNick { nick: target.clone() }).await
        }
    }
}

pub async fn handle_quit(_: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error> {
    let client = client.read().await;
    let reason = msg.params.get(0).map(|str| str.to_owned()).unwrap_or_else(|| "Quit".to_owned());
    if let ClientStatus::Unregistered{..} = client.status {
        return Err(Error::new(ErrorKind::Other, reason.clone()));
    }

    client.broadcast(Message {
        tags: Vec::new(),
        source: Some(client.get_extended_prefix().unwrap()),
        command: "QUIT".to_owned(),
        params: vec!(reason.clone()),
    }, true).await?;

    let mut channels = client.channels.write().await;
    channels.clear();

    // We return an "error" to signal the quit
    Err(Error::new(ErrorKind::Other, reason.clone()))
}
