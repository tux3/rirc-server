use crate::client::Client;
use crate::server::ServerState;
use crate::channel::{Channel, Topic};
use crate::message::{Message, make_reply_msg, ReplyCode};
use crate::errors::ChannelNotFoundError;
use crate::commands::command_error;
use chrono::Local;
use std::io::Error;
use std::collections::hash_map::{Entry};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::error::Error as _;

pub async fn handle_join(state: Arc<ServerState>, client_lock: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error> {
    let client = client_lock.read().await;

    let chanlist = match msg.params.get(0) {
        Some(chanlist) => chanlist.split(','),
        None => return command_error(&state, &client, ReplyCode::ErrNeedMoreParams{cmd: "JOIN".to_owned()}).await,
    };

    for chan_name in chanlist {
        if !chan_name.starts_with('#') {
            command_error(&state, &client, ReplyCode::ErrNoSuchChannel{channel: chan_name.to_string()}).await?;
            continue;
        }

        let client = client_lock.read().await;
        if client.channels.read().await.len() >= state.settings.chan_limit {
            command_error(&state, &client, ReplyCode::ErrTooManyChannels{channel: chan_name.to_owned()}).await?;
            break;
        }

        let mut channels = state.channels.lock().await;
        let channel_arc = match channels.entry(chan_name.to_ascii_uppercase()) {
            Entry::Occupied(entry) => entry.get().clone(),
            Entry::Vacant(entry) => {
                if !state.settings.allow_channel_creation {
                    command_error(&state, &client, ReplyCode::ErrNoSuchChannel{channel: chan_name.to_owned()}).await?;
                    continue;
                }
                entry.insert(Arc::new(RwLock::new(Channel::new(chan_name.to_owned())))).clone()
            },
        };

        {
            let mut client_chans_guard = client.channels.write().await;
            match client_chans_guard.entry(chan_name.to_ascii_uppercase()) {
                Entry::Occupied(_) => continue,
                Entry::Vacant(entry) => {
                    entry.insert(Arc::downgrade(&channel_arc));
                },
            };
        }

        let channel_guard = channel_arc.read().await;
        let client_nick = &client.get_nick().unwrap();
        let msgs = &channel_guard.get_join_msgs(&state, client_nick).await;
        client.send_all(msgs).await?;
        let mut chan_users_guard = channel_guard.users.write().await;
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
            let chan_user_guard = chan_user.read().await;
            chan_user_guard.send(join_msg.clone()).await?;
        }
        drop(chan_users_guard);
    };

    Ok(())
}

pub async fn handle_part(state: Arc<ServerState>, client_lock: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error> {
    let client = client_lock.read().await;

    let chanlist = match msg.params.get(0) {
        Some(chanlist) => chanlist.split(','),
        None => return command_error(&state, &client, ReplyCode::ErrNeedMoreParams{cmd: "PART".to_owned()}).await,
    };

    let mut futs = Vec::new();
    for chan_name in chanlist {
        if !chan_name.starts_with('#') {
            command_error(&state, &client, ReplyCode::ErrNoSuchChannel{channel: chan_name.to_string()}).await?;
        } else {
            futs.push(client.part(chan_name));
        }
    }

    let nick = &client.get_nick().unwrap();
    for result in futures::future::join_all(futs).await {
        let err = match result {
            Ok(()) => continue,
            Err(err) => err,
        };

        if err.source().is_some() && err.source().unwrap().is::<ChannelNotFoundError>() {
            let chan_err = err.into_inner().unwrap().downcast::<ChannelNotFoundError>().unwrap();
            client.send(make_reply_msg(&state, nick, ReplyCode::ErrNotOnChannel { channel: chan_err.channel })).await?;
        } else {
            return Err(err);
        };
    }

    Ok(())
}

pub async fn handle_topic(state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error> {
    let client = client.read().await;
    let target_chan = match msg.params.get(0) {
        Some(target_chan) => target_chan,
        None => return command_error(&state, &client, ReplyCode::ErrNeedMoreParams{cmd: "TOPIC".to_owned()}).await,
    };
    let topic_text = msg.params.get(1);

    if let Some(channel_ref) = state.channels.lock().await.get(&target_chan.to_ascii_uppercase()) {
        let channel_lock = channel_ref.clone();
        let mut channel_guard = channel_lock.write().await;
        let channel = channel_guard.name.clone();

        if let Some(text) = topic_text {
            if text.is_empty() {
                channel_guard.topic = None;
            } else {
                channel_guard.topic = Some(Topic {
                    text: text.clone(),
                    set_by_host: client.get_extended_prefix().unwrap(),
                    set_at: Local::now(),
                });
            }
            channel_guard.send(Message{
                tags: Vec::new(),
                source: Some(client.get_extended_prefix().expect("TOPIC change by user without a prefix!")),
                command: "TOPIC".to_owned(),
                params: vec!(channel, text.to_owned()),
            }, None).await?;
        } else {
            let client_nick = client.get_nick().unwrap();
            if let Some(ref topic) = channel_guard.topic {
                client.send_all(&[
                    make_reply_msg(&state, &client_nick, ReplyCode::RplTopic { channel: channel.clone(), text: topic.text.clone() }),
                    make_reply_msg(&state, &client_nick, ReplyCode::RplTopicWhoTime { channel, who: topic.set_by_host.clone(), time: topic.set_at }),
                ]).await?;
            } else {
                client.send(make_reply_msg(&state, &client_nick, ReplyCode::RplNoTopic { channel })).await?;
            }
        }
    } else {
        command_error(&state, &client, ReplyCode::ErrNoSuchChannel{channel: target_chan.clone()}).await?;
    };

    Ok(())
}