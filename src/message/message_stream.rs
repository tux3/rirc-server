use futures::{ready, Stream};
use std::io::Error;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, Lines};

use crate::message::Message;
use std::pin::Pin;
use std::task::{Context, Poll};

// A Stream for receiving IRC messages
#[must_use = "streams do nothing unless polled"]
pub struct MessageStream<T: AsyncRead + AsyncBufRead + Unpin> {
    lines: Lines<T>,
}

impl<T: AsyncRead + AsyncBufRead + Unpin> MessageStream<T> {
    pub fn new(io: T) -> MessageStream<T> {
        MessageStream { lines: io.lines() }
    }
}

impl<T: AsyncRead + AsyncBufRead + Unpin> Stream for MessageStream<T> {
    type Item = Result<Message, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = Pin::into_inner(self);
        let line = ready!(Pin::new(&mut this.lines).poll_next_line(cx))?;
        let line = line.map(|s| Message::new(&s));
        Poll::Ready(Ok(line).transpose())
    }
}
