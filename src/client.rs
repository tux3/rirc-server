use std::net::SocketAddr;
use std::io::{Error, BufReader};
use tokio::net::TcpStream;
use tokio::io::{AsyncRead};
use message::Message;
use message_sink::MessageSink;
use message_stream::MessageStream;
use futures::{Stream, Sink, Future};

pub enum ClientStatus {
    Unidentified, // State immediately after connecting, before setting Nick and User
    User, // Normal user
}

pub struct ClientDuplex {
    pub stream: Box<Stream<Item=Message, Error=Error> + Send>,
    pub client: Client,
}

impl ClientDuplex {
    pub fn new(socket: TcpStream) -> ClientDuplex {
        let addr = socket.peer_addr().unwrap();
        let (socket_r, socket_w) = socket.split();
        let stream = Box::new(MessageStream::new(BufReader::new(socket_r)));
        let sink = Box::new(MessageSink::new(socket_w));
        ClientDuplex {
            stream,
            client: Client {
                sink,
                addr,
                state: ClientStatus::Unidentified,
            },
        }
    }
}

pub struct Client {
    sink: Box<Sink<SinkItem=Message, SinkError=Error> + Send>,
    pub addr: SocketAddr,
    pub state: ClientStatus,
}

impl Client {
    pub fn send(self, msg: Message) -> Box<Future<Item=Client, Error=Error> + Send> {
        let Client{sink, addr, state} = self;
        Box::new(sink.send(msg).and_then(move |sink| {
            Ok(Client{
                sink,
                addr,
                state,
            })
        }))
    }
}