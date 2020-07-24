use std::fmt::{Display, Formatter, Error};

#[derive(Debug)]
pub struct ChannelNotFoundError {
    pub channel: String
}

impl ChannelNotFoundError {
    pub fn new(channel: String) -> Self {
        Self {
            channel
        }
    }
}

impl Display for ChannelNotFoundError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        write!(f, "{}", self.channel)
    }
}

impl std::error::Error for ChannelNotFoundError {

}
