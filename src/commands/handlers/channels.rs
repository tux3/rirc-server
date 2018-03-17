use client::Client;
use server::ServerState;
use channel::{Channel};
use message::{Message, make_reply_msg, ReplyCode};
use futures::{Future, future};
use std::io::{Error};
use std::collections::hash_map::{Entry};
use std::sync::{Arc, RwLock};

pub fn handle_join(state: Arc<ServerState>, client_lock: Arc<RwLock<Client>>, msg: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    let client = client_lock.read().expect("Client read lock broken");
    let mut send_futs = Vec::new();
    let chanlist = match msg.params.get(0) {
        Some(chanlist) => chanlist.split(','),
        None => return command_error!(state, client, ReplyCode::ErrNeedMoreParams{cmd: "JOIN".to_owned()}),
    }.filter(|chan_name| {
        if !chan_name.starts_with("#") {
            send_futs.push(command_error!(state, client, ReplyCode::ErrNoSuchChannel{channel: chan_name.to_string()}));
            false
        } else {
            true
        }
    }).collect::<Vec<_>>();
    drop(client);

    for chan_name in chanlist {
        let mut client = client_lock.write().expect("Client write lock broken");
        if client.channels.read().unwrap().len() >= state.settings.chan_limit {
            send_futs.push(command_error!(state, client, ReplyCode::ErrTooManyChannels{channel: chan_name.to_owned()}));
            break;
        }

        let mut channels = state.channels.lock().expect("Channels lock broken");
        let channel_arc = match channels.entry(chan_name.to_ascii_uppercase()) {
            Entry::Occupied(entry) => entry.get().clone(),
            Entry::Vacant(entry) => {
                if !state.settings.allow_channel_creation {
                    send_futs.push(command_error!(state, client, ReplyCode::ErrNoSuchChannel{channel: chan_name.to_owned()}));
                    continue;
                }
                entry.insert(Arc::new(RwLock::new(Channel::new(chan_name.to_owned())))).clone()
            },
        };

        {
            let mut client_chans_guard = client.channels.write().expect("Client channels write lock broken");
            match client_chans_guard.entry(chan_name.to_ascii_uppercase()) {
                Entry::Occupied(_) => continue,
                Entry::Vacant(entry) => {
                    entry.insert(Arc::downgrade(&channel_arc)).clone();
                },
            };
        }

        let channel_guard = channel_arc.read().expect("Channel read lock broken");
        let mut chan_users_guard = channel_guard.users.write().expect("Channel users lock broken");
        chan_users_guard.insert(client.addr.to_string(), Arc::downgrade(&client_lock));

        let join_msg = Message {
            tags: Vec::new(),
            source: Some(client.get_extended_prefix().expect("JOIN sent by user without a prefix!")),
            command: "JOIN".to_owned(),
            params: vec!(channel_guard.name.to_owned()),
        };
        drop(client);

        for chan_user_weak in chan_users_guard.values() {
            let chan_user = match chan_user_weak.upgrade() {
                Some(user) => user,
                None => continue,
            };
            let chan_user_guard = chan_user.read().expect("Chan user read lock broken");
            send_futs.push(chan_user_guard.send(join_msg.clone()));
        }
        drop(chan_users_guard);

        let client = client_lock.read().expect("Client read lock broken");
        send_futs.push(client.send_all(&channel_guard.get_join_msgs(&state, &client.get_nick().unwrap())));
    }

    Box::new(future::join_all(send_futs).map(|_| ()))
}

pub fn handle_part(state: Arc<ServerState>, client_lock: Arc<RwLock<Client>>, msg: Message) -> Box<Future<Item=(), Error=Error>  + Send> {
    let client = client_lock.read().expect("Client read lock broken");
    let mut send_futs = Vec::new();
    let chanlist = match msg.params.get(0) {
        Some(chanlist) => chanlist.split(','),
        None => return command_error!(state, client, ReplyCode::ErrNeedMoreParams{cmd: "JOIN".to_owned()}),
    }.filter(|chan_name| {
        if !chan_name.starts_with("#") {
            send_futs.push(command_error!(state, client, ReplyCode::ErrNoSuchChannel{channel: chan_name.to_string()}));
            false
        } else {
            true
        }
    }).collect::<Vec<_>>();

    for chan_name in chanlist {
        send_futs.push(client.part(chan_name));
    }

    Box::new(future::join_all(send_futs).map(|_| ()))
}