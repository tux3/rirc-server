use crate::client::{Client, ClientStatus};
use crate::server::ServerState;
use crate::message::{Message, ReplyCode, make_reply_msg};
use futures::{Future};
use std::io::{Error};
use std::collections::HashMap;
use std::sync::Arc;
use std::pin::Pin;
use tokio::sync::RwLock;
use lazy_static::lazy_static;

macro_rules! pub_use_submodules {
    ( $( $name:ident ),* ) => {
        $(
            mod $name;
            pub use self::$name::*;
        )*
    };
}

pub_use_submodules!(misc, identity, channels, userqueries);

enum CommandNamespace {
    /// Clients in any state can execute this command
    Any,
    /// Command can be used by normal users after registration
    Normal,
}

type CommandHandlerFuture = Pin<Box<dyn Future<Output=Result<(), Error>> + Send>>;
pub type CommandHandler = fn(Arc<ServerState>, Arc<RwLock<Client>>, Message) -> CommandHandlerFuture;

pub struct Command {
    pub name: &'static str,
    permissions: CommandNamespace,
    pub handler: CommandHandler,
}

macro_rules! declare_commands {
    ( pub const $cmd_list:ident = [ $( { $cmd:pat, $namespace:expr }, )* ] ) => {

        pub const $cmd_list : &[Command] = &[
            $( Command {
                name: paste::expr! { stringify!( [<$cmd:upper>] ) },
                permissions: $namespace,
                handler: paste::expr! { [<handle_ $cmd _thunk>] }
            } ),*
        ];

        $( paste::item! {
            fn [<handle_ $cmd _thunk>](state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> CommandHandlerFuture {
                Box::pin( [<handle_ $cmd>] (state, client, msg))
            }
        } )*
    };
}

declare_commands!(
    pub const COMMANDS_LIST = [
        {ping, CommandNamespace::Any},
        {nick, CommandNamespace::Any},
        {user, CommandNamespace::Any},
        {notice, CommandNamespace::Normal},
        {version, CommandNamespace::Normal},
        {lusers, CommandNamespace::Normal},
        {motd, CommandNamespace::Normal},
        {privmsg, CommandNamespace::Normal},
        {join, CommandNamespace::Normal},
        {part, CommandNamespace::Normal},
        {quit, CommandNamespace::Normal},
        {topic, CommandNamespace::Normal},
        {who, CommandNamespace::Normal},
        {whois, CommandNamespace::Normal},
        {mode, CommandNamespace::Normal},
        {names, CommandNamespace::Normal},
    ]
);

lazy_static! {
    pub static ref COMMANDS: HashMap<&'static str, &'static Command> = {
        let mut m = HashMap::new();
        for cmd in COMMANDS_LIST {
            m.insert(cmd.name, cmd);
        }
        m
    };
}

/// Sending an error reply (only if the client has a nick)
pub async fn command_error(state: &ServerState, client: &Client, err: ReplyCode) -> Result<(), Error> {
    if let Some(nick) = client.get_nick() {
        client.send(make_reply_msg(state, &nick, err)).await?
    }

    Ok(())
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
