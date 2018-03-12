use client::{Client, ClientStatus};
use server::ServerState;
use message::Message;
use reply_codes::{make_reply_msg, ReplyCode};
use futures::{Future, future};
use regex::Regex;
use std::io::{Error};
use std::collections::HashMap;
use std::sync::Arc;

enum CommandNamespace {
    /// Clients in any state can execute this command
    Any,
    /// Command can be used by normal users after registration
    Normal,
}

pub type CommandHandler = fn(Arc<ServerState>, &mut Client, Message) -> Box<Future<Item=(), Error=Error>  + Send>;

pub struct Command {
    pub name: &'static str,
    permissions: CommandNamespace,
    pub handler: CommandHandler,
}

const COMMANDS_LIST: &[Command] = &[
    Command{name: "NICK", permissions: CommandNamespace::Any, handler: handle_nick},
    Command{name: "USER", permissions: CommandNamespace::Any, handler: handle_user},
    Command{name: "NOTICE", permissions: CommandNamespace::Any, handler: handle_notice},
    Command{name: "VERSION", permissions: CommandNamespace::Normal, handler: handle_version},
    Command{name: "LUSERS", permissions: CommandNamespace::Normal, handler: handle_lusers},
    Command{name: "MOTD", permissions: CommandNamespace::Normal, handler: handle_motd},
    Command{name: "PRIVMSG", permissions: CommandNamespace::Normal, handler: handle_privmsg},
];

lazy_static! {
    pub static ref COMMANDS: HashMap<&'static str, &'static Command> = {
        let mut m = HashMap::new();
        for cmd in COMMANDS_LIST {
            m.insert(cmd.name, cmd);
        }
        m
    };

    static ref VALID_NICKNAME_REGEX: Regex = Regex::new(r"^[[:alpha:]\[\\\]\^_`\{\|\}][[:alnum:]\[\\\]\^_`\{\|\}\-]*$").unwrap();
    static ref BAD_USERNAME_CHARS_REGEX: Regex = Regex::new(r"[@\x00\x0D\x0A\x20]").unwrap();
}

/// Sending an error reply if the client has a nick
macro_rules! command_error {
    ( $state:expr, $client:expr, $err:expr ) => {
        {
            match $client.get_nick() {
                Some(nick) => Box::new($client.send(make_reply_msg(&$state, &nick, $err))),
                None => Box::new(future::ok(())),
            }
        }
    };
}

fn is_valid_nick(max_len: usize, nick: &str) -> bool {
    !nick.is_empty()
        && nick.len() <= max_len
        && VALID_NICKNAME_REGEX.is_match(nick)
}

fn make_valid_username(max_len: usize, username: &str) -> Option<String> {
    let mut username = username.to_owned();
    username.truncate(max_len-1);
    if let Some(mat) = BAD_USERNAME_CHARS_REGEX.find(&username).and_then(|mat| Some(mat.start())) {
        username.truncate(mat);
    };
    if !username.is_empty() {
        Some("~".to_owned()+&username)
    } else {
        None
    }
}

pub fn is_command_available(cmd: &Command, client: &Client) -> bool {
    match cmd.permissions {
        CommandNamespace::Any => true,
        CommandNamespace::Normal => match client.status {
            ClientStatus::Normal(_) => true,
            _ => false,
        },
    }
}

pub fn handle_nick(state: Arc<ServerState>, client: &mut Client, msg: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    let old_extended_prefix = client.get_extended_prefix();
    let new_nick = match msg.params.get(0) {
        Some(nick) => nick,
        None => return command_error!(state, client, ReplyCode::ErrNoNicknameGiven),
    };
    if !is_valid_nick(state.settings.max_name_length, new_nick) {
        let cur_nick = client.get_nick().unwrap_or("*".to_owned());
        return client.send(make_reply_msg(&state, &cur_nick, ReplyCode::ErrErroneusNickname{nick: new_nick.clone()}));
    }

    if state.users.lock().expect("State users lock broken").contains_key(&new_nick.to_ascii_uppercase()) {
        return command_error!(state, client, ReplyCode::ErrNicknameInUse{nick: new_nick.clone()});
    }

    match client.status {
        ClientStatus::Unregistered(ref mut state) => state.nick = Some(new_nick.clone()),
        ClientStatus::Normal(ref mut state) => state.nick = new_nick.clone(),
    };

    return if let ClientStatus::Unregistered{..} = client.status {
        client.try_finish_registration(state.clone())
    } else if old_extended_prefix.is_some() {
        client.send(Message {
            tags: Vec::new(),
            source: old_extended_prefix,
            command: "NICK".to_owned(),
            params: vec!(new_nick.clone()),
        })
    } else {
        Box::new(future::ok(()))
    }
}

pub fn handle_user(state: Arc<ServerState>, client: &mut Client, msg: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    let username = match msg.params.get(0) {
        Some(username) => match make_valid_username(state.settings.max_name_length, username) {
            Some(username) => username,
            None => {
                let nick = client.get_nick().unwrap_or("*".to_owned());
                client.send(Message {
                    tags: Vec::new(),
                    source: Some(state.settings.server_name.clone()),
                    command: "NOTICE".to_owned(),
                    params: vec!(nick, "*** Your username is invalid. Please make sure that your username contains only alphanumeric characters.".to_owned()),
                });
                return client.close_with_error( "Invalid username");
            },
        },
        None => return command_error!(state, client, ReplyCode::ErrNeedMoreParams{cmd: msg.command}),
    };
    let realname = match msg.params.get(3) {
        Some(realname) => realname,
        None => return command_error!(state, client, ReplyCode::ErrNeedMoreParams{cmd: msg.command}),
    };

    match client.status {
        ClientStatus::Unregistered(ref mut client_state) => {
            client_state.username = Some(username.clone());
            client_state.realname = Some(realname.clone());
        },
        _ => return command_error!(state, client, ReplyCode::ErrAlreadyRegistered),
    };

    client.try_finish_registration(state)
}

pub fn handle_notice(_: Arc<ServerState>, _: &mut Client, _: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    Box::new(future::ok(()))
}

pub fn handle_version(state: Arc<ServerState>, client: &mut Client, msg: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    if let Some(target) = msg.params.get(0) {
        if target != &state.settings.server_name {
            return command_error!(state, client, ReplyCode::ErrNoSuchServer{server: target.clone()});
        }
    };

    let nick = client.get_nick().unwrap_or("*".to_owned());
    client.send(make_reply_msg(&state, &nick, ReplyCode::RplVersion {comments: String::new()}));
    client.send_issupport(&state)
}

pub fn handle_lusers(state: Arc<ServerState>, client: &mut Client, msg: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    if let Some(target) = msg.params.get(0) {
        if target != &state.settings.server_name {
            return command_error!(state, client, ReplyCode::ErrNoSuchServer{server: target.clone()});
        }
    };

    client.send_lusers(&state)
}

pub fn handle_motd(state: Arc<ServerState>, client: &mut Client, msg: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    if let Some(target) = msg.params.get(0) {
        if target != &state.settings.server_name {
            return command_error!(state, client, ReplyCode::ErrNoSuchServer{server: target.clone()});
        }
    };

    client.send_motd(&state)
}

pub fn handle_privmsg(state: Arc<ServerState>, client: &mut Client, msg: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    let target = match msg.params.get(0) {
        Some(nick) => nick,
        None => return command_error!(state, client, ReplyCode::ErrNoRecipient{cmd: "PRIVMSG".to_owned()}),
    };
    let msg_text = match msg.params.get(1) {
        Some(msg_text) => msg_text,
        None => return command_error!(state, client, ReplyCode::ErrNoTextToSend),
    };
    let reply = Message {
        tags: Vec::new(),
        source: Some(client.get_extended_prefix().expect("PRIVMSG sent by user without a prefix!")),
        command: "PRIVMSG".to_owned(),
        params: vec!(msg_text.to_owned()),
    };

    // TODO: If the target starts with #, treat it as a channel

    if target.to_ascii_uppercase() == client.get_nick().expect("PRIVMSG sent by user without a nick!").to_ascii_uppercase() {
        client.send(reply)
    } else if let Some(target_user) = state.users.lock().expect("State users lock broken").get(&target.to_ascii_uppercase()) {
        let target_user = match target_user.upgrade() {
            Some(target_user) => target_user,
            None => return command_error!(state, client, ReplyCode::ErrNoSuchNick{nick: target.clone()}),
        };
        let target_user = target_user.read().expect("User read lock broken");
        target_user.send(reply)
    } else {
        return command_error!(state, client, ReplyCode::ErrNoSuchNick{nick: target.clone()});
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            assert!(names.insert(cmd.name), format!("Command {} appears twice in the list", cmd.name));
            assert!(handlers.insert(cmd.handler as usize), format!("Command {}'s handler is a duplicate", cmd.name));
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