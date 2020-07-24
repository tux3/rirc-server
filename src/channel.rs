use crate::client::{Client};
use crate::message::{Message, ReplyCode, make_reply_msg};
use crate::server::ServerState;
use tokio::sync::RwLock;
use std::sync::Weak;
use std::collections::HashMap;
use futures::FutureExt;
use std::io::Error;
use futures::future;
use chrono::{DateTime, Local};
use futures::executor::block_on;

pub struct Topic {
    pub text: String,
    pub set_by_host: String,
    pub set_at: DateTime<Local>,
}

pub struct Channel {
    pub name: String, // Includes the # character
    pub topic: Option<Topic>,
    pub users: RwLock<HashMap<String, Weak<RwLock<Client>>>>, // Client addr -> chan member
}

impl Channel {
    pub fn new(name: String) -> Channel {
        Channel {
            name,
            topic: None,
            users: RwLock::new(HashMap::new()),
        }
    }

    /// Get a series of info messages to send after a client joins a channel
    /// Call this before adding the user to the channel, or the user's nick will appear twice!
    pub async fn get_join_msgs(&self, state: &ServerState, client_nick: &str) -> Vec<Message> {
        let mut msgs = Vec::new();
        if let Some(ref topic) = self.topic {
            msgs.push(make_reply_msg(state, client_nick,
                                     ReplyCode::RplTopic{channel: self.name.clone(), text: topic.text.clone()}));
            msgs.push(make_reply_msg(state, client_nick,
                                     ReplyCode::RplTopicWhoTime{channel: self.name.clone(), who: topic.set_by_host.clone(), time: topic.set_at}));
        }

        let users_guard = self.users.read().await;
        let mut names = users_guard.values().map(|user| {
            user.upgrade().and_then(|user| {
                block_on(user.read()).get_nick()
            })
        })
            .filter(|name_opt| name_opt.is_some())
            .map(|name_opt| name_opt.unwrap())
            .collect::<Vec<_>>();
        names.push(client_nick.to_owned());
        let base_msg = make_reply_msg(state, client_nick, ReplyCode::RplNameReply{symbol: '=', channel: self.name.clone()});

        msgs.extend(Message::split_trailing_args(base_msg, names, " "));
        msgs.push(make_reply_msg(state, client_nick, ReplyCode::RplEndOfNames{channel: self.name.clone()}));
        msgs
    }

    /// Sends a message to all members of a channel
    pub async fn send(&self, message: Message, exclude_user_addr: Option<String>) -> Result<(), Error> {
        let users_guard = self.users.read().await;
        let mut futs = Vec::new();
        for user in users_guard.values() {
            let user = match user.upgrade() {
                Some(user) => user,
                None => continue,
            };

            let exclude_user_addr = exclude_user_addr.clone();
            let message = message.clone();
            futs.push(async move {
                let user_guard = user.read().await;
                if exclude_user_addr.is_none() || exclude_user_addr.as_ref().unwrap() != &user_guard.addr.to_string() {
                    user_guard.send(message).boxed().await?;
                }
                Result::<(), Error>::Ok(())
            })
        };
        let results = future::join_all(futs).await;
        for result in results {
            result?;
        }
        Ok(())
    }
}
