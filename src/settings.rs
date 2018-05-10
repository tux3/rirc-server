use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct ServerSettings {
    /// Network address/port to listen on
    pub listen_addr: SocketAddr,
    /// Name the server will use to identify itself
    pub server_name: String,
    /// Advertised network name for this server
    pub network_name: String,
    /// Maximum length of nicknames and usernames
    /// Note that the madatory leading "~" in usernames counts towards this limit
    pub max_name_length: usize,
    /// Maximum length of a channel name
    pub max_channel_length: usize,
    /// Maximum length of a channel topic
    pub max_topic_length: usize,
    /// Maximum number of #channels a client may join
    pub chan_limit: usize,
    /// Whether regular users can create channels
    pub allow_channel_creation: bool,
}

impl Default for ServerSettings {
    fn default() -> Self {
        ServerSettings{
            listen_addr: "0.0.0.0:6667".parse().unwrap(),
            server_name: "rirc-server".to_owned(),
            network_name: "rIRC".to_owned(),
            max_name_length: 16,
            max_channel_length: 50,
            max_topic_length: 390,
            chan_limit: 120,
            allow_channel_creation: true,
        }
    }
}