use crate::client::{Client};
use crate::server::ServerState;
use crate::message::{Message, make_reply_msg, ReplyCode};
use crate::commands::command_error;
use std::io::{Error};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::{HashSet};

fn who_reply_for_user(state: &ServerState, asker_nick: &str, chan_name: String, user: &Client) -> Message {
    make_reply_msg(&state, asker_nick, ReplyCode::RplWhoReply{
        channel: chan_name,
        user: user.get_username().unwrap(),
        host: user.get_host(),
        server: state.settings.server_name.clone(),
        nick: user.get_nick().unwrap(),
        status: 'H', // I believe H means Here, and G is Gone/Away
        hopcount: 0,
        realname: user.get_realname().unwrap(),
    })
}

fn user_matches_mask(user: &Client, mask: &str) -> bool {
    // TODO: Handle wildcards
    if user.get_nick().unwrap() == mask {
        true
    } else {
        false
    }
}

pub async fn handle_who(state: Arc<ServerState>, client: Arc<RwLock<Client>>, msg: Message) -> Result<(), Error> {
    let client = client.read().await;
    let mask = match msg.params.get(0) {
        Some(mask) => mask,
        None => return command_error(&state, &client, ReplyCode::ErrNeedMoreParams{cmd: "WHO".to_owned()}).await,
    };
    let op_param = msg.params.get(1);
    if let Some(_) = op_param {
        // TODO: If and when we add operators, the /who op param should be implemented
        return command_error(&state, &client, ReplyCode::RplEndOfWho{mask: mask.to_owned()}).await;
    }

    let mut messages = Vec::new();
    if let Some(channel_ref) = state.channels.lock().await.get(&mask.to_ascii_uppercase()) {
        let channel_lock = channel_ref.clone();
        let channel_guard = channel_lock.read().await;
        let channel_users_guard = channel_guard.users.read().await;

        for weak_user in channel_users_guard.values() {
            let user_lock = match weak_user.upgrade() {
                Some(user) => user,
                None => continue,
            };
            let user_guard = user_lock.read().await;
            messages.push(who_reply_for_user(&state, &client.get_nick().unwrap(), channel_guard.name.clone(), &user_guard))
        }
    } else {
        let mut users_matched = HashSet::new();
        for channel_weak in client.channels.read().await.values() {
            let channel_lock = match channel_weak.upgrade() {
                Some(channel) => channel,
                None => continue,
            };
            let channel_guard = channel_lock.read().await;

            let channel_users = channel_guard.users.read().await;
            for (user_addr, weak_user) in channel_users.iter() {
                if !users_matched.insert(user_addr.to_string()) {
                    continue
                }

                let user_lock = match weak_user.upgrade() {
                    Some(user) => user,
                    None => continue,
                };
                let user_guard = user_lock.read().await;
                if !user_matches_mask(&user_guard, &mask) {
                    continue
                }
                messages.push(who_reply_for_user(&state, &client.get_nick().unwrap(), channel_guard.name.clone(), &user_guard))
            }
        }
    }

    messages.push(make_reply_msg(&state, &client.get_nick().unwrap(), ReplyCode::RplEndOfWho{mask: mask.to_owned()}));
    client.send_all(&messages).await
}
