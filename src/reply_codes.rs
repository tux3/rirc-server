use message::Message;
use server::ServerState;

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

    RplVersion{comments: String},

    ErrNoSuchNick{nick: String},
    ErrNoSuchServer{server: String},
    ErrNoSuchChannel{channel: String},
    ErrNoRecipient{cmd: String},
    ErrNoTextToSend,
    ErrUnknownCommand{cmd: String},
    ErrNoMotd,
    ErrNoNicknameGiven,
    ErrErroneusNickname{nick: String},
    ErrNicknameInUse{nick: String},
    ErrNeedMoreParams{cmd: String},
    ErrAlreadyRegistered,
}

pub fn make_reply_msg(state: &ServerState, client_nick: &str, reply_type: ReplyCode) -> Message {
    let (cmd_num, mut params, description) = match reply_type {
        ReplyCode::RplWelcome => ("001", vec!() , format!("Welcome to the {} Internet Relay Chat Network {}", state.settings.network_name, client_nick)),
        ReplyCode::RplYourHost => ("002", vec!() , format!("Your host is {}, running version {}", state.settings.server_name, env!("CARGO_PKG_VERSION"))),
        ReplyCode::RplCreated => ("003", vec!() , format!("This server was created {}", state.creation_time)),
        ReplyCode::RplMyInfo => ("004", vec!(state.settings.server_name.clone(), env!("CARGO_PKG_VERSION").to_owned()) , format!("")),
        ReplyCode::RplIsSupport{features} => ("005", features , format!("are supported by this server")),

        ReplyCode::RplLuserClient{num_visibles, num_invisibles} => ("251", vec!(),  format!("There are {} users and {} invisible on 1 servers", num_visibles, num_invisibles)),
        ReplyCode::RplLuserOp{num_ops} => ("252", vec!(num_ops.to_string()),  format!("operator(s) online")),
        ReplyCode::RplLuserUnknown{num_unknowns} => ("253", vec!(num_unknowns.to_string()),  format!("unknown connection(s)")),
        ReplyCode::RplLuserChannels{num_channels} => ("254", vec!(num_channels.to_string()),  format!("channels formed")),
        ReplyCode::RplLuserMe{num_users} => ("255", vec!(),  format!("I have {} clients and 1 servers", num_users)),
        ReplyCode::RplLocalUsers{num_users, max_users_seen} => ("265", vec!(num_users.to_string(), max_users_seen.to_string()),
                                                                   format!("Current local users {}, max {}", num_users, max_users_seen)),
        ReplyCode::RplGlobalUsers{num_users, max_users_seen} => ("266", vec!(num_users.to_string(), max_users_seen.to_string()),
                                                                    format!("Current global users {}, max {}", num_users, max_users_seen)),

        ReplyCode::RplVersion{comments} => ("351", vec!(env!("CARGO_PKG_VERSION").to_owned(), state.settings.server_name.clone()),  comments),

        ReplyCode::ErrNoSuchNick{nick} => ("401", vec!(nick) , format!("No such nick/channel")),
        ReplyCode::ErrNoSuchServer{server} => ("402", vec!(server) , format!("No such server")),
        ReplyCode::ErrNoSuchChannel{channel} => ("403", vec!(channel) , format!("No such channel")),
        ReplyCode::ErrNoRecipient{cmd} => ("411", vec!() , format!("No recipient given ({})", cmd)),
        ReplyCode::ErrNoTextToSend => ("412", vec!() , format!("No text to send")),
        ReplyCode::ErrUnknownCommand{cmd} => ("421", vec!(cmd) , format!("Unknown command")),
        ReplyCode::ErrNoMotd => ("422", vec!() , format!("No MOTD set.")),
        ReplyCode::ErrNoNicknameGiven => ("431", vec!() , format!("No nickname given")),
        ReplyCode::ErrErroneusNickname{nick} => ("432", vec!(nick) , format!("Erroneous nickname")),
        ReplyCode::ErrNicknameInUse{nick} => ("433", vec!(nick) , format!("Nickname is already in use.")),
        ReplyCode::ErrNeedMoreParams{cmd} => ("461", vec!(cmd) , format!("Not enough parameters")),
        ReplyCode::ErrAlreadyRegistered => ("462", vec!() , format!("You may not reregister")),
    };

    params.insert(0, client_nick.to_owned());
    params.push(description.to_owned());
    Message {
        tags: Vec::new(),
        source: Some(state.settings.server_name.clone()),
        command: cmd_num.to_owned(),
        params,
    }
}