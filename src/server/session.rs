use crate::{
    protocol::{self as proto, ChatMessage},
    server::{self, ClientRequestTransaction, ServerResponse},
};
use futures::{
    AsyncWrite,
    channel::{mpsc, oneshot},
};
use futures::{Sink, SinkExt as _, Stream, StreamExt as _};
use tracing::{error, info, instrument};

/// Represents the operations that a Hotline server must support. Anything
/// implementing this trait should be able to power a Session and handle clients.
pub trait Server: Clone + Send + 'static {
    fn send_chat(&self, msg: proto::ChatMessage) -> impl Future<Output = ()> + Send;
    fn post_news(&self, msg: proto::PostNews) -> impl Future<Output = ()> + Send;
    fn get_news(&self) -> impl Future<Output = Vec<proto::Message>> + Send;
    fn download_file(
        &self,
        msg: proto::DownloadFile,
    ) -> impl Future<Output = proto::DownloadFileReply> + Send;
    fn get_file(
        &self,
        path: proto::FilePath,
        name: proto::FileName,
    ) -> impl Future<Output = proto::FlattenedFileObject> + Send;
}

pub trait SessionStream: Stream<Item = ClientRequestTransaction> + Unpin + Send + 'static {}
pub trait SessionSink: Sink<OutgoingMessage> + Unpin + Send + 'static {}

impl<R: Stream<Item = ClientRequestTransaction> + Unpin + Send + 'static> SessionStream for R {}
impl<R: Sink<OutgoingMessage> + Unpin + Send + 'static> SessionSink for R {}

/// These will always know their ID since either the state of the session
/// defines it or it's a reply to an incoming client request.
pub enum OutgoingMessage {
    Event(proto::Id, server::ServerRequest),
    Reply(proto::Id, server::ServerResponse),
}

/// These will only know their ID if they're responding to an incoming request.
enum QueuedMessage {
    Event(server::ServerRequest),
    Reply(proto::Id, server::ServerResponse),
}

/// This is how peers of the session can interact with the session. For example,
/// if you want to send a message to a user, send a chat notifciation to a
/// handle associated with the user's session.
///
/// TODO: This might need to return Results in many of its methods.
#[derive(Clone)]
pub struct SessionHandle {
    id: u32,
    tx: mpsc::Sender<QueuedMessage>,
}

impl SessionHandle {
    pub async fn notify(&mut self, event: server::ServerRequest) {
        let _ = self.tx.send(QueuedMessage::Event(event)).await;
    }
    pub async fn reply(&mut self, id: proto::Id, reply: server::ServerResponse) {
        let _ = self.tx.send(QueuedMessage::Reply(id, reply)).await;
    }
    pub fn id(&self) -> u32 {
        self.id
    }
}

/// Represents a connection from a client that's managed by the server.
/// It doesn't know anything about the Hotline network protocol. It reads in
/// requests, replies to them, and sends out-of-band events.
/// All logic is delegated to the inner server.
pub struct Session<R, W, S: Server> {
    id: u32,
    server: S,
    reader: R,
    writer: W,
    out_tx: mpsc::Sender<QueuedMessage>,
    out_rx: mpsc::Receiver<QueuedMessage>,
}

impl<R: SessionStream, W: SessionSink, S: Server> Session<R, W, S> {
    pub fn new(id: u32, server: S, reader: R, writer: W) -> Self {
        let (out_tx, out_rx) = mpsc::channel(1);
        Self {
            id,
            server,
            reader,
            writer,
            out_tx,
            out_rx,
        }
    }
    pub fn handle(&self) -> SessionHandle {
        SessionHandle {
            id: self.id,
            tx: self.out_tx.clone(),
        }
    }
    #[instrument(level = "info", name = "session", skip(self), fields(id = self.id))]
    pub async fn run(self) {
        let handle = self.handle();
        let Self {
            reader,
            writer,
            server,
            out_rx,
            out_tx,
            ..
        } = self;

        drop(out_tx);

        info!("starting session");

        let r = ReadLoop {
            server,
            stream: reader,
            handle,
        };

        let w = WriteLoop {
            stream: writer,
            outgoing: out_rx,
        };

        let (r, r_cancel) = futures::future::abortable(r.run());
        let (w, w_cancel) = futures::future::abortable(w.run());

        if let Err(e) = futures::try_join!(r, w) {
            r_cancel.abort();
            w_cancel.abort();
            error!("connection error: {e:?}");
        }

        info!("done");
    }
}

struct ReadLoop<R: SessionStream, S: Server> {
    server: S,
    stream: R,
    handle: SessionHandle,
}

impl<R: SessionStream, S: Server> ReadLoop<R, S> {
    #[instrument(name = "rx", skip(self))]
    async fn run(self) -> proto::ProtocolResult<()> {
        info!("starting session reader");
        let Self {
            mut stream,
            server,
            mut handle,
        } = self;
        while let Some(ClientRequestTransaction { id, body }) = stream.next().await {
            match body {
                server::ClientRequest::DownloadFile(req) => {
                    let reply = server.download_file(req).await;
                    handle
                        .reply(id, ServerResponse::DownloadFileReply(reply))
                        .await;
                }
                server::ClientRequest::PostNews(req) => {
                    server.post_news(req).await;
                }
                server::ClientRequest::GetMessages(..) => {
                    let news = server.get_news().await;
                    let msg = proto::GetMessagesReply::with_messages(news);
                    handle
                        .reply(id, server::ServerResponse::GetMessagesReply(msg))
                        .await;
                }
                server::ClientRequest::SendChat(req) => {
                    let msg = ChatMessage {
                        chat_id: req.chat_id,
                        message: req.message,
                    };
                    server.send_chat(msg).await;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

struct WriteLoop<S: SessionSink> {
    stream: S,
    outgoing: mpsc::Receiver<QueuedMessage>,
}

impl<S: SessionSink> WriteLoop<S> {
    #[instrument(name = "tx", skip(self))]
    async fn run(self) -> proto::ProtocolResult<()> {
        let Self {
            mut stream,
            mut outgoing,
            ..
        } = self;
        info!("starting session writer");
        let mut event_id = 0u32;
        while let Some(msg) = outgoing.next().await {
            let msg = match msg {
                QueuedMessage::Event(event) => {
                    let id = event_id;
                    event_id = event_id.wrapping_add(1);
                    info!(id = id, "event! {event:?}");
                    OutgoingMessage::Event(id.into(), event)
                }
                QueuedMessage::Reply(id, reply) => {
                    let id_num = u32::from(id);
                    info!(id = id_num, "reply! {reply:?}");
                    OutgoingMessage::Reply(id, reply)
                }
            };
            if stream.send(msg).await.is_err() {
                return Err(proto::ProtocolError::SystemError);
            }
        }
        Ok(())
    }
}
