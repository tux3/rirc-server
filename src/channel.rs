use client::{Client};
use message::{Message, ReplyCode, make_reply_msg};
use std::sync::{Weak, RwLock};
use server::ServerState;
use std::collections::HashMap;
use futures::Future;
use std::io::Error;
use futures::future;

pub struct Channel {
    pub name: String, // Includes the # character
    pub users: RwLock<HashMap<String, Weak<RwLock<Client>>>>, // Client addr -> chan member
}

impl Channel {
    pub fn new(name: String) -> Channel {
        Channel {
            name,
            users: RwLock::new(HashMap::new()),
        }
    }

    pub fn get_join_msgs(&self, state: &ServerState, client_nick: &str) -> Vec<Message> {
        let mut msgs = Vec::new();
        let users_guard = self.users.read().expect("Channel users read lock broken");
        let names = users_guard.values().map(|user| {
            user.upgrade().and_then(|user| {
                user.read().unwrap().get_nick()
            })
        })
            .filter(|name_opt| name_opt.is_some())
            .map(|name_opt| name_opt.unwrap())
            .collect::<Vec<_>>();
        let base_msg = make_reply_msg(state, client_nick, ReplyCode::RplNameReply{symbol: '=', channel: self.name.clone()});

        msgs.extend(Message::split_trailing_args(base_msg, names, " "));
        msgs.push(make_reply_msg(state, client_nick, ReplyCode::RplEndOfNames{channel: self.name.clone()}));
        msgs
    }

    pub fn send(&self, message: Message, exclude_user_addr: Option<String>) -> Box<Future<Item=(), Error=Error>  + Send> {
        let users_guard = self.users.read().expect("Channel users lock broken");
        let futs = users_guard.values().map(|user| {
            let user = match user.upgrade() {
                Some(user) => user,
                None => return Box::new(future::ok(())) as Box<Future<Item=(), Error=Error>  + Send>,
            };
            let user_guard = user.read().expect("User read lock broken");
            if exclude_user_addr.is_some() && exclude_user_addr.as_ref().unwrap() == &user_guard.addr.to_string() {
                Box::new(future::ok(()))
            } else {
                user_guard.send(message.clone())
            }
        });
        Box::new(future::join_all(futs.collect::<Vec<_>>()).map(|_| ()))
    }
}