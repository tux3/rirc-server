use tokio::io::{AsyncRead, AsyncBufRead, AsyncBufReadExt};
use std::io::{Error};
use futures::{Stream, ready, FutureExt};

use crate::message::Message;
use std::task::{Context, Poll};
use std::pin::Pin;

// A Stream for receiving IRC messages
#[must_use = "streams do nothing unless polled"]
pub struct MessageStream<T: AsyncRead + AsyncBufRead + Unpin> {
    io: T,
    msg_line: String,
}

impl<T: AsyncRead + AsyncBufRead + Unpin> MessageStream<T> {
    pub fn new(io: T) -> MessageStream<T> {
        MessageStream {
            io,
            msg_line: String::new(),
        }
    }
}

impl<T: AsyncRead + AsyncBufRead + Unpin> Stream for MessageStream<T> {
    type Item = Result<Message, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = Pin::into_inner(self);

        let mut line_fut = this.io.read_line(&mut this.msg_line);

        let n = match ready!(line_fut.poll_unpin(cx)) {
            Ok(n) => n,
            Err(e) => return Poll::Ready(Some(Err(e))),
        };
        if n == 0 && this.msg_line.len() == 0 {
            return Poll::Ready(None)
        }
        let msg = Message::new(&this.msg_line);
        this.msg_line.clear();
        Poll::Ready(Some(Ok(msg)))
    }
}
