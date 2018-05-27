use std::error::Error;
use std::net::SocketAddr;
use client::Client;
use channel::Channel;
use message::Message;

pub struct ServerCallbacks {
    // A new client just connected, doesn't have a nick/user yet. Return true to accept it.
    pub on_client_connect: fn(&SocketAddr) -> Result<bool, Box<Error>>,
    // A client is trying to register (setting their nick/user). Return true to accept it.
    pub on_client_registering: fn(&mut Client) -> Result<bool, Box<Error>>,
    // A client has completed registration, received the MOTD, and can now be sent extra commands.
    pub on_client_registered: fn(&mut Client) -> Result<(), Box<Error>>,
    // A client disconnected. The client may or may not have completed registration.
    pub on_client_disconnect: fn(&SocketAddr) -> Result<(), Box<Error>>,
    // A registered client is sending a message on a channel, return true to accept it.
    pub on_client_channel_message: fn(&Client, &Channel, &Message) -> Result<bool, Box<Error>>,
}

impl Default for ServerCallbacks {
    fn default() -> Self {
        ServerCallbacks{
            on_client_connect: |_| Ok(true),
            on_client_registering: |_| Ok(true),
            on_client_registered: |_| Ok(()),
            on_client_disconnect: |_| Ok(()),
            on_client_channel_message: |_,_,_| Ok(true),
        }
    }
}
