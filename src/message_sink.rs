use tokio::io::{AsyncWrite};
use std::io::{Error};
use futures::{Sink, Poll, Async, AsyncSink, StartSend};
use message::Message;

// A Sink for sending IRC messages
pub struct MessageSink<T: AsyncWrite> {
    io: T,
    send_buffer: Vec<u8>,
}

impl<T: AsyncWrite> MessageSink<T> {
    pub fn new(io: T) -> MessageSink<T> {
        MessageSink {
            io,
            send_buffer: Vec::new(),
        }
    }
}

impl<T: AsyncWrite> Sink for MessageSink<T> {
    type SinkItem = Message;
    type SinkError = Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.send_buffer.extend_from_slice(item.to_line().as_bytes());

        self.poll_complete().map(|_| AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), Error> {
        if let Async::Ready(n) = self.io.poll_write(&self.send_buffer)? {
            self.send_buffer.drain(0..n);
        };

        Ok(if self.send_buffer.is_empty() {
            Async::Ready(())
        } else {
            Async::NotReady
        })
    }

    fn close(&mut self) -> Poll<(), Error> {
        self.poll_complete()
    }
}
