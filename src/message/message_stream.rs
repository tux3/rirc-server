use tokio::io::{AsyncRead, AsyncBufRead, AsyncBufReadExt, Lines};
use std::io::{Error};
use futures::{Stream, ready};

use crate::message::Message;
use std::task::{Context, Poll};
use std::pin::Pin;

// A Stream for receiving IRC messages
#[must_use = "streams do nothing unless polled"]
pub struct MessageStream<T: AsyncRead + AsyncBufRead + Unpin> {
    lines: Lines<T>,
}

impl<T: AsyncRead + AsyncBufRead + Unpin> MessageStream<T> {
    pub fn new(io: T) -> MessageStream<T> {
        MessageStream {
            lines: io.lines(),
        }
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
