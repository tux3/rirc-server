use crate::message::Message;
use futures::task::{Context, Poll};
use futures::Sink;
use std::io::Error;
use tokio::io::AsyncWrite;
use tokio::macros::support::Pin;

// A Sink for sending IRC messages
pub struct MessageSink<T: AsyncWrite + Unpin> {
    io: Pin<Box<T>>,
    send_buffer: Vec<u8>,
}

impl<T: AsyncWrite + Unpin> MessageSink<T> {
    pub fn new(io: T) -> MessageSink<T> {
        MessageSink {
            io: Box::pin(io),
            send_buffer: Vec::new(),
        }
    }
}

impl<T: AsyncWrite + Unpin> Sink<Message> for MessageSink<T> {
    type Error = Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.poll_flush(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        self.send_buffer
            .extend_from_slice(item.to_line().as_bytes());
        self.send_buffer.push('\n' as u8);
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = Pin::into_inner(self);

        while !this.send_buffer.is_empty() {
            match this.io.as_mut().poll_write(cx, &this.send_buffer) {
                Poll::Ready(Ok(n)) => {
                    this.send_buffer.drain(0..n);
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
                Poll::Pending => return Poll::Pending,
            }
        }

        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.poll_flush(cx)
    }
}
