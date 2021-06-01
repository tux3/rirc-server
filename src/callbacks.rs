use crate::channel::Channel;
use crate::client::Client;
use crate::message::Message;
use std::error::Error;
use std::net::SocketAddr;

type CallbackResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct ServerCallbacks {
    // A new client just connected, doesn't have a nick/user yet. Return true to accept it.
    pub on_client_connect: fn(&SocketAddr) -> CallbackResult<bool>,
    // A client is trying to register (setting their nick/user). Return true to accept it.
    pub on_client_registering: fn(&mut Client) -> CallbackResult<bool>,
    // A client has completed registration, received the MOTD, and can now be sent extra commands.
    pub on_client_registered: fn(&Client) -> CallbackResult<()>,
    // A client disconnected. The client may or may not have completed registration.
    pub on_client_disconnect: fn(&SocketAddr) -> CallbackResult<()>,
    // A registered client is sending a message on a channel, return true to accept it.
    pub on_client_channel_message: fn(&Client, &Channel, &Message) -> CallbackResult<bool>,
}

impl Default for ServerCallbacks {
    fn default() -> Self {
        ServerCallbacks {
            on_client_connect: |_| Ok(true),
            on_client_registering: |_| Ok(true),
            on_client_registered: |_| Ok(()),
            on_client_disconnect: |_| Ok(()),
            on_client_channel_message: |_, _, _| Ok(true),
        }
    }
}
