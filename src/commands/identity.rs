use crate::client::{Client, ClientStatus};
use crate::server::ServerState;
use crate::message::{Message, make_reply_msg, ReplyCode};
use crate::commands::command_error;
use regex::Regex;
use std::io::Error;
use std::sync::Arc;
use tokio::sync::RwLock;
use lazy_static::lazy_static;

lazy_static! {
    static ref VALID_NICKNAME_REGEX: Regex = Regex::new(r"^[[:alpha:]\[\\\]\^_`\{\|\}][[:alnum:]\[\\\]\^_`\{\|\}\-]*$").unwrap();
    static ref BAD_USERNAME_CHARS_REGEX: Regex = Regex::new(r"[@\x00\x0D\x0A\x20]").unwrap();
}

fn is_valid_nick(max_len: usize, nick: &str) -> bool {
    !nick.is_empty()
        && nick.len() <= max_len
        && VALID_NICKNAME_REGEX.is_match(nick)
}

fn make_valid_username(max_len: usize, username: &str) -> Option<String> {
    let mut username = username.to_owned();
    username.truncate(max_len-1);
    if let Some(mat) = BAD_USERNAME_CHARS_REGEX.find(&username).map(|mat| mat.start()) {
        username.truncate(mat);
    };
    if !username.is_empty() {
        Some("~".to_owned()+&username)
    } else {
        None
    }
}

pub async fn handle_nick(state: Arc<ServerState>, client_lock: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error> {
    let mut client = client_lock.write().await;
    let new_nick = match msg.params.get(0) {
        Some(nick) => nick,
        None => return command_error(&state, &client, ReplyCode::ErrNoNicknameGiven).await,
    };
    if !is_valid_nick(state.settings.max_name_length, new_nick) {
        let cur_nick = client.get_nick().unwrap_or_else(|| "*".to_owned());
        return client.send(make_reply_msg(&state, &cur_nick, ReplyCode::ErrErroneusNickname{nick: new_nick.clone()})).await;
    }

    if state.users.read().await.contains_key(&new_nick.to_ascii_uppercase()) {
        return command_error(&state, &client, ReplyCode::ErrNicknameInUse{nick: new_nick.clone()}).await;
    }

    let old_extended_prefix = client.get_extended_prefix();
    let old_nick = client.get_nick();

    match client.status {
        ClientStatus::Unregistered(ref mut state) => state.nick = Some(new_nick.clone()),
        ClientStatus::Normal(ref mut state) => state.nick = new_nick.clone(),
    };

    if let ClientStatus::Unregistered{..} = client.status {
        let should_finish = client.try_begin_registration().await?;
        drop(client);
        if should_finish {
            client_lock.read().await.finish_registration().await?;
        }
        Ok(())
    } else {
        drop(client);
        let client = client_lock.read().await;

        let mut users_map = state.users.write().await;
        let old_user = users_map.remove(&old_nick.unwrap().to_ascii_uppercase());
        users_map.insert(new_nick.to_ascii_uppercase(), old_user.unwrap());

        client.broadcast(Message {
            tags: Vec::new(),
            source: old_extended_prefix,
            command: "NICK".to_owned(),
            params: vec!(new_nick.clone()),
        }, true).await
    }
}

pub async fn handle_user(state: Arc<ServerState>, client_lock: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error> {
    let mut client = client_lock.write().await;
    let username = match msg.params.get(0) {
        Some(username) => match make_valid_username(state.settings.max_name_length, username) {
            Some(username) => username,
            None => {
                let nick = client.get_nick().unwrap_or_else(|| "*".to_owned());
                client.send(Message {
                    tags: Vec::new(),
                    source: Some(state.settings.server_name.clone()),
                    command: "NOTICE".to_owned(),
                    params: vec!(nick, "*** Your username is invalid. Please make sure that your username contains only alphanumeric characters.".to_owned()),
                }).await?;
                return client.close_with_error( "Invalid username").await;
            },
        },
        None => return command_error(&state, &client, ReplyCode::ErrNeedMoreParams{cmd: msg.command}).await,
    };
    let realname = match msg.params.get(3) {
        Some(realname) => realname,
        None => return command_error(&state, &client, ReplyCode::ErrNeedMoreParams{cmd: msg.command}).await,
    };

    match client.status {
        ClientStatus::Unregistered(ref mut client_state) => {
            client_state.username = Some(username.clone());
            client_state.realname = Some(realname.clone());
        },
        _ => return command_error(&state, &client, ReplyCode::ErrAlreadyRegistered).await,
    };

    let should_finish = client.try_begin_registration().await?;
    drop(client);
    if should_finish {
        client_lock.read().await.finish_registration().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::COMMANDS_LIST;
    use std::collections::HashSet;

    fn is_valid_username(max_len: usize, username: &str) -> bool {
        match make_valid_username(max_len, username) {
            Some(fixed) => fixed == "~".to_owned()+username,
            None =>  false,
        }
    }

    #[test]
    fn no_command_duplicates() {
        let mut names = HashSet::new();
        let mut handlers = HashSet::new();
        for cmd in COMMANDS_LIST {
            assert!(names.insert(cmd.name), "Command {} appears twice in the list", cmd.name);
            assert!(handlers.insert(cmd.handler as usize), "Command {}'s handler is a duplicate", cmd.name);
        }
    }

    #[test]
    fn nicks_length() {
        assert_eq!(is_valid_nick(4, ""), false);
        assert_eq!(is_valid_nick(4, "x"), true);
        assert_eq!(is_valid_nick(4, "xx"), true);
        assert_eq!(is_valid_nick(4, "xxxx"), true);
        assert_eq!(is_valid_nick(4, "xxxxx"), false);

        assert_eq!(is_valid_nick(8, ""), false);
        assert_eq!(is_valid_nick(8, "x"), true);
        assert_eq!(is_valid_nick(8, "xxxx"), true);
        assert_eq!(is_valid_nick(8, "xxxxxxxx"), true);
        assert_eq!(is_valid_nick(8, "xxxxxxxxx"), false);
    }

    #[test]
    fn nicks_charset() {
        assert_eq!(is_valid_nick(16, "abcxyz"), true);
        assert_eq!(is_valid_nick(16, "ABCXYZ"), true);
        assert_eq!(is_valid_nick(16, "aaa555"), true);
        assert_eq!(is_valid_nick(16, "555aaa"), false);
        assert_eq!(is_valid_nick(16, "#channel"), false);

        assert_eq!(is_valid_nick(16, "aaa---"), true);
        assert_eq!(is_valid_nick(16, "---aaa"), false);

        assert_eq!(is_valid_nick(16, r"[{|\`^_-}]"), true);

        assert_eq!(is_valid_nick(16, "abc def"), false);
        assert_eq!(is_valid_nick(16, "abc!def"), false);
        assert_eq!(is_valid_nick(16, "abc@def"), false);
        assert_eq!(is_valid_nick(16, "abc#def"), false);
        assert_eq!(is_valid_nick(16, "abc$def"), false);
        assert_eq!(is_valid_nick(16, "abc%def"), false);
        assert_eq!(is_valid_nick(16, "abc&def"), false);
        assert_eq!(is_valid_nick(16, "abc*def"), false);
        assert_eq!(is_valid_nick(16, "abc(def"), false);
        assert_eq!(is_valid_nick(16, "abc)def"), false);
        assert_eq!(is_valid_nick(16, "abc+def"), false);
    }

    #[test]
    fn username_length() {
        assert_eq!(is_valid_username(4, ""), false);
        assert_eq!(is_valid_username(4, "x"), true);
        assert_eq!(is_valid_username(4, "xx"), true);
        assert_eq!(is_valid_username(4, "xxx"), true);
        assert_eq!(is_valid_username(4, "xxxx"), false);

        assert_eq!(is_valid_username(8, ""), false);
        assert_eq!(is_valid_username(8, "x"), true);
        assert_eq!(is_valid_username(8, "xxxx"), true);
        assert_eq!(is_valid_username(8, "xxxxxxx"), true);
        assert_eq!(is_valid_username(8, "xxxxxxxx"), false);
    }

    #[test]
    fn username_charset() {
        assert_eq!(is_valid_username(16, "abcxyz"), true);
        assert_eq!(is_valid_username(16, "ABCXYZ"), true);
        assert_eq!(is_valid_username(16, "aaa555"), true);
        assert_eq!(is_valid_username(16, "555aaa"), true);

        assert_eq!(is_valid_username(16, "aaa---"), true);
        assert_eq!(is_valid_username(16, "---aaa"), true);

        assert_eq!(is_valid_username(16, r"[{|\`^_-}]"), true);
        assert_eq!(is_valid_username(16, r"-!<#$~%;&*():+?"), true);

        assert_eq!(is_valid_username(16, r"abc def"), false);
        assert_eq!(is_valid_username(16, r"abc@def"), false);
        assert_eq!(is_valid_username(16, "abc\0def"), false);
        assert_eq!(is_valid_username(16, "abc\ndef"), false);
        assert_eq!(is_valid_username(16, "abc\rdef"), false);
    }
}
