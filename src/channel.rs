use crate::client::Client;
use crate::message::{make_reply_msg, Message, ReplyCode};
use crate::mode::ChannelMode;
use crate::server::ServerState;
use chrono::{DateTime, Local};
use futures::future;
use futures::FutureExt;
use std::collections::HashMap;
use std::io::Error;
use std::sync::Weak;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

pub struct Topic {
    pub text: String,
    pub set_by_host: String,
    pub set_at: DateTime<Local>,
}

pub struct Channel {
    pub name: String, // Includes the # character
    pub topic: Option<Topic>,
    pub users: RwLock<HashMap<String, Weak<RwLock<Client>>>>, // Client addr -> chan member
    pub creation_timestamp: u64,
    pub mode: ChannelMode,
}

impl Channel {
    pub fn new(name: String) -> Channel {
        Channel {
            name,
            topic: None,
            users: RwLock::new(HashMap::new()),
            creation_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            mode: Default::default(),
        }
    }

    pub async fn get_names_msgs(&self, state: &ServerState, client_nick: &str) -> Vec<Message> {
        let mut msgs = Vec::new();
        let users_guard = self.users.read().await;

        let mut names = Vec::new();
        for weak_user in users_guard.values() {
            if let Some(user) = weak_user.upgrade() {
                if let Some(nick) = user.read().await.get_nick() {
                    names.push(nick);
                }
            }
        }

        let base_msg = make_reply_msg(
            state,
            client_nick,
            ReplyCode::RplNameReply {
                symbol: '=',
                channel: self.name.clone(),
            },
        );
        msgs.extend(Message::split_trailing_args(base_msg, names, " "));
        msgs.push(make_reply_msg(
            state,
            client_nick,
            ReplyCode::RplEndOfNames {
                channel: self.name.clone(),
            },
        ));
        msgs
    }

    /// Get a series of info messages to send after a client joins a channel
    /// Call this right after adding the user to the channel
    pub async fn get_join_msgs(&self, state: &ServerState, client_nick: &str) -> Vec<Message> {
        let mut msgs = Vec::new();
        if let Some(ref topic) = self.topic {
            msgs.push(make_reply_msg(
                state,
                client_nick,
                ReplyCode::RplTopic {
                    channel: self.name.clone(),
                    text: topic.text.clone(),
                },
            ));
            msgs.push(make_reply_msg(
                state,
                client_nick,
                ReplyCode::RplTopicWhoTime {
                    channel: self.name.clone(),
                    who: topic.set_by_host.clone(),
                    time: topic.set_at,
                },
            ));
        }

        msgs.append(&mut self.get_names_msgs(state, client_nick).await);
        msgs
    }

    /// Sends a message to all members of a channel
    pub async fn send(
        &self,
        message: Message,
        exclude_user_addr: Option<String>,
    ) -> Result<(), Error> {
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
                if exclude_user_addr.is_none()
                    || exclude_user_addr.as_ref().unwrap() != &user_guard.addr.to_string()
                {
                    user_guard.send(message).boxed().await?;
                }
                Result::<(), Error>::Ok(())
            })
        }
        let results = future::join_all(futs).await;
        for result in results {
            result?;
        }
        Ok(())
    }
}
