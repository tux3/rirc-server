use client::{Client, ClientStatus};
use server::ServerState;
use message::Message;
use futures::{Future};
use std::io::{Error};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use commands::*;

enum CommandNamespace {
    /// Clients in any state can execute this command
    Any,
    /// Command can be used by normal users after registration
    Normal,
}

pub type CommandHandler = fn(Arc<ServerState>, Arc<RwLock<Client>>, Message) -> Box<Future<Item=(), Error=Error>  + Send>;

pub struct Command {
    pub name: &'static str,
    permissions: CommandNamespace,
    pub handler: CommandHandler,
}

pub const COMMANDS_LIST: &[Command] = &[
    Command{name: "PING", permissions: CommandNamespace::Any, handler: handle_ping},
    Command{name: "NICK", permissions: CommandNamespace::Any, handler: handle_nick},
    Command{name: "USER", permissions: CommandNamespace::Any, handler: handle_user},
    Command{name: "NOTICE", permissions: CommandNamespace::Any, handler: handle_notice},
    Command{name: "VERSION", permissions: CommandNamespace::Normal, handler: handle_version},
    Command{name: "LUSERS", permissions: CommandNamespace::Normal, handler: handle_lusers},
    Command{name: "MOTD", permissions: CommandNamespace::Normal, handler: handle_motd},
    Command{name: "PRIVMSG", permissions: CommandNamespace::Normal, handler: handle_privmsg},
    Command{name: "JOIN", permissions: CommandNamespace::Normal, handler: handle_join},
    Command{name: "PART", permissions: CommandNamespace::Normal, handler: handle_part},
    Command{name: "QUIT", permissions: CommandNamespace::Normal, handler: handle_quit},
    Command{name: "TOPIC", permissions: CommandNamespace::Normal, handler: handle_topic},
];

lazy_static! {
    pub static ref COMMANDS: HashMap<&'static str, &'static Command> = {
        let mut m = HashMap::new();
        for cmd in COMMANDS_LIST {
            m.insert(cmd.name, cmd);
        }
        m
    };
}

/// Sending an error reply if the client has a nick
macro_rules! command_error {
    ( $state:expr, $client:expr, $err:expr ) => {
        match $client.get_nick() {
            Some(nick) => Box::new($client.send(make_reply_msg(&$state, &nick, $err))),
            None => Box::new(future::ok(())) as Box<Future<Item=(), Error=Error>  + Send>,
        }
    };
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
