use crate::message::Message;
use crate::server::ServerState;
use chrono::{DateTime, Local};

#[allow(dead_code)] // Some reply codes may not be used yet, but that's ok: they're from the spec
pub enum ReplyCode {
    RplWelcome,
    RplYourHost,
    RplCreated,
    RplMyInfo,
    RplIsSupport{features: Vec<String>},

    RplLuserClient{num_visibles: usize, num_invisibles: usize},
    RplLuserOp{num_ops: usize},
    RplLuserUnknown{num_unknowns: usize},
    RplLuserChannels{num_channels: usize},
    RplLuserMe{num_users: usize},
    RplLocalUsers{num_users: usize, max_users_seen: usize},
    RplGlobalUsers{num_users: usize, max_users_seen: usize},

    RplEndOfWho{mask: String},
    RplNoTopic{channel: String},
    RplTopic{channel: String, text: String},
    RplTopicWhoTime{channel: String, who: String, time: DateTime<Local>},
    RplVersion{comments: String},
    RplWhoReply{channel: String, user: String, host: String, server: String, nick: String, status: char, hopcount: u32, realname: String},
    /// This is a base reply, it does not include names since they may not fit in a single message.
    RplNameReply{symbol: char, channel: String},
    RplEndOfNames{channel: String},

    ErrNoSuchNick{nick: String},
    ErrNoSuchServer{server: String},
    ErrNoSuchChannel{channel: String},
    ErrCannotSendToChan{channel: String, reason: String},
    ErrTooManyChannels{channel: String},
    ErrNoRecipient{cmd: String},
    ErrNoTextToSend,
    ErrUnknownCommand{cmd: String},
    ErrNoMotd,
    ErrNoNicknameGiven,
    ErrErroneusNickname{nick: String},
    ErrNicknameInUse{nick: String},
    ErrNotOnChannel{channel: String},
    ErrNeedMoreParams{cmd: String},
    ErrAlreadyRegistered,
}

pub fn make_reply_msg(state: &ServerState, client_nick: &str, reply_type: ReplyCode) -> Message {
    let (cmd_num, mut params, description) = match reply_type {
        ReplyCode::RplWelcome => ("001", vec!() , Some(format!("Welcome to the {} Internet Relay Chat Network {}", state.settings.network_name, client_nick))),
        ReplyCode::RplYourHost => ("002", vec!() , Some(format!("Your host is {}, running version {}", state.settings.server_name, env!("CARGO_PKG_VERSION")))),
        ReplyCode::RplCreated => ("003", vec!() , Some(format!("This server was created {}", state.creation_time))),
        ReplyCode::RplMyInfo => ("004", vec!(state.settings.server_name.clone(), env!("CARGO_PKG_VERSION").to_owned()), None),
        ReplyCode::RplIsSupport{features} => ("005", features, Some(format!("are supported by this server"))),

        ReplyCode::RplLuserClient{num_visibles, num_invisibles} => ("251", vec!(), Some(format!("There are {} users and {} invisible on 1 servers", num_visibles, num_invisibles))),
        ReplyCode::RplLuserOp{num_ops} => ("252", vec!(num_ops.to_string()), Some(format!("operator(s) online"))),
        ReplyCode::RplLuserUnknown{num_unknowns} => ("253", vec!(num_unknowns.to_string()), Some(format!("unknown connection(s)"))),
        ReplyCode::RplLuserChannels{num_channels} => ("254", vec!(num_channels.to_string()), Some(format!("channels formed"))),
        ReplyCode::RplLuserMe{num_users} => ("255", vec!(), Some(format!("I have {} clients and 1 servers", num_users))),
        ReplyCode::RplLocalUsers{num_users, max_users_seen} => ("265", vec!(num_users.to_string(), max_users_seen.to_string()),
                                                                    Some(format!("Current local users {}, max {}", num_users, max_users_seen))),
        ReplyCode::RplGlobalUsers{num_users, max_users_seen} => ("266", vec!(num_users.to_string(), max_users_seen.to_string()),
                                                                    Some(format!("Current global users {}, max {}", num_users, max_users_seen))),

        ReplyCode::RplEndOfWho{mask} => ("315", vec!(mask), Some(format!("End of WHO list"))),
        ReplyCode::RplNoTopic{channel} => ("331", vec!(channel), Some(format!("No topic is set"))),
        ReplyCode::RplTopic{channel, text} => ("332", vec!(channel), Some(text)),
        ReplyCode::RplTopicWhoTime{channel, who, time} => ("333", vec!(channel, who, time.timestamp().to_string()), None),
        ReplyCode::RplVersion{comments} => ("351", vec!(env!("CARGO_PKG_VERSION").to_owned(), state.settings.server_name.clone()), Some(comments)),
        ReplyCode::RplWhoReply{channel, user, host, server, nick, status, hopcount, realname} =>
                                            ("352", vec!(channel, user, host, server, nick, status.to_string()), Some(format!("{} {}", hopcount, realname))),
        ReplyCode::RplNameReply{symbol, channel} => ("353", vec!(symbol.to_string(), channel), None),
        ReplyCode::RplEndOfNames{channel} => ("366", vec!(channel), Some(format!("End of /NAMES list"))),

        ReplyCode::ErrNoSuchNick{nick} => ("401", vec!(nick) , Some(format!("No such nick/channel"))),
        ReplyCode::ErrNoSuchServer{server} => ("402", vec!(server) , Some(format!("No such server"))),
        ReplyCode::ErrNoSuchChannel{channel} => ("403", vec!(channel) , Some(format!("No such channel"))),
        ReplyCode::ErrCannotSendToChan{channel, reason} => ("404", vec!(channel), Some(reason)),
        ReplyCode::ErrTooManyChannels{channel} => ("405", vec!(channel) , Some(format!("You have joined too many channels"))),
        ReplyCode::ErrNoRecipient{cmd} => ("411", vec!() , Some(format!("No recipient given ({})", cmd))),
        ReplyCode::ErrNoTextToSend => ("412", vec!() , Some(format!("No text to send"))),
        ReplyCode::ErrUnknownCommand{cmd} => ("421", vec!(cmd) , Some(format!("Unknown command"))),
        ReplyCode::ErrNoMotd => ("422", vec!() , Some(format!("No MOTD set."))),
        ReplyCode::ErrNoNicknameGiven => ("431", vec!() , Some(format!("No nickname given"))),
        ReplyCode::ErrErroneusNickname{nick} => ("432", vec!(nick) , Some(format!("Erroneous nickname"))),
        ReplyCode::ErrNicknameInUse{nick} => ("433", vec!(nick) , Some(format!("Nickname is already in use."))),
        ReplyCode::ErrNotOnChannel {channel} => ("442", vec!(channel) , Some(format!("You're not on that channel"))),
        ReplyCode::ErrNeedMoreParams{cmd} => ("461", vec!(cmd) , Some(format!("Not enough parameters"))),
        ReplyCode::ErrAlreadyRegistered => ("462", vec!() , Some(format!("You may not reregister"))),
    };

    params.insert(0, client_nick.to_owned());
    if let Some(description) = description {
        params.push(description);
    }
    Message {
        tags: Vec::new(),
        source: Some(state.settings.server_name.clone()),
        command: cmd_num.to_owned(),
        params,
    }
}
