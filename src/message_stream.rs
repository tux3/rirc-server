use tokio::io::{AsyncRead, ErrorKind};
use std::io::{BufRead, Error};
use futures::{Stream, Poll, Async};
use std::mem;
use message::Message;

// A Stream for receiving IRC messages
#[must_use = "streams do nothing unless polled"]
pub struct MessageStream<T: AsyncRead + BufRead> {
    io: T,
    msg_line: String,
}

impl<T: AsyncRead + BufRead> MessageStream<T> {
    pub fn new(io: T) -> MessageStream<T> {
        MessageStream {
            io,
            msg_line: String::new(),
        }
    }
}

impl<T: AsyncRead + BufRead> Stream for MessageStream<T> {
    type Item = Message;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let n = match self.io.read_line(&mut self.msg_line) {
            Ok(t) => t,
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                return Ok(Async::NotReady)
            },
            Err(e) => return Err(e.into()),
        };
        if n == 0 && self.msg_line.len() == 0 {
            return Ok(None.into())
        }
        let msg_line = &mem::replace(&mut self.msg_line, String::new());
        Ok(Some(Message::new(msg_line)).into())
    }
}
