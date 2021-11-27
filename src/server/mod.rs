use tokio::io::AsyncRead;
use futures::stream::{
    TryStreamExt as _,
    Stream,
    select,
};
use tokio::sync::broadcast::{
    self,
    Sender,
    Receiver,
    error::{SendError, RecvError, TryRecvError},
};
use async_stream::stream;

use derive_more::{From, Into};

use thiserror::Error;

pub mod bus;
pub mod files;
pub mod users;
pub mod chat;
pub mod transaction_stream;

use crate::protocol::{
    TransactionFrame,
    ProtocolError,
    ChatId,
    ChatMessage,
    UserNameWithInfo,
    ServerMessage,
};

use transaction_stream::Frames;

#[derive(Debug, Clone)]
pub enum Message {
    TransactionReceived(TransactionFrame),
    Chat(Chat),
    ChatRoomJoin(ChatRoomPresence),
    ChatRoomLeave(ChatRoomPresence),
    Broadcast(Broadcast),
    InstantMessage(InstantMessage),
    UserConnect(User),
    UserUpdate(User),
    UserDisconnect(User),
}

#[derive(Debug)]
pub struct Bus {
    sender: Sender<Message>,
    receiver: Receiver<Message>,
}

#[derive(Debug, Error)]
pub enum BusError {
    #[error("dropped {0} messages from sender")]
    Lagged(u64),
    #[error("channel is closed")]
    Closed,
}

impl <T> From<SendError<T>> for BusError {
    fn from(_: SendError<T>) -> Self {
        Self::Closed
    }
}

impl From<RecvError> for BusError {
    fn from(error: RecvError) -> Self {
        match error {
            RecvError::Closed => Self::Closed,
            RecvError::Lagged(n) => Self::Lagged(n),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Chat(pub Option<ChatId>, pub User, pub Vec<u8>);

impl Into<ChatMessage> for Chat {
    fn into(self) -> ChatMessage {
        let Self(chat_id, user, text) = self;
        let username = user.0.username.take();
        let message = [
            &b"\r "[..],
            &username[..],
            &b": "[..],
            &text[..],
        ].concat();
        ChatMessage {
            chat_id,
            message,
        }
    }
}

#[derive(Debug, Clone, From, Into)]
pub struct User(pub UserNameWithInfo);

impl Into<UserId> for User {
    fn into(self) -> UserId {
        self.0.user_id
    }
}

#[derive(Debug, Clone, From, Into)]
pub struct ChatRoomPresence(pub ChatId, pub User);

#[derive(Debug, Clone)]
pub struct ChatRoomChat(pub ChatId, pub User, pub Vec<u8>);

impl Into<ChatMessage> for ChatRoomChat {
    fn into(self) -> ChatMessage {
        let Self(chat_id, user, text) = self;
        let chat_id = Some(chat_id);
        let chat = Chat(user, text);
        ChatMessage { chat_id, ..chat.into() }
    }
}

#[derive(Debug, Clone)]
pub struct InstantMessage {
    pub from: User,
    pub to: User,
    pub message: Vec<u8>,
}

pub type BusResult<T> = Result<T, BusError>;

impl Bus {
    pub fn new(capacity: usize) -> Self {
        let (sender, receiver) = broadcast::channel(capacity);
        Self { sender, receiver }
    }
    pub fn chat(&mut self, chat: Chat) -> BusResult<()> {
        self.sender.send(Message::Chat(chat))?;
        Ok(())
    }
    pub fn broadcast(&mut self, broadcast: Broadcast) -> BusResult<()> {
        self.sender.send(Message::Broadcast(broadcast))?;
        Ok(())
    }
    pub fn instant_message(
        &mut self,
        message: InstantMessage,
    ) -> BusResult<()> {
        self.sender.send(Message::InstantMessage(message))?;
        Ok(())
    }
    pub fn user_connect(&mut self, user: User) -> BusResult<()> {
        self.sender.send(Message::UserConnect(user))?;
        Ok(())
    }
    pub fn user_update(&mut self, user: User) -> BusResult<()> {
        self.sender.send(Message::UserUpdate(user))?;
        Ok(())
    }
    pub fn user_disconnect(&mut self, user: User) -> BusResult<()> {
        self.sender.send(Message::UserDisconnect(user))?;
        Ok(())
    }
    pub fn chat_room_join(&mut self, chat: ChatId, user: User) -> BusResult<()> {
        self.sender.send(Message::ChatRoomJoin(ChatRoomPresence(chat, user)))?;
        Ok(())
    }
    pub fn chat_room_leave(&mut self, chat: ChatId, user: User) -> BusResult<()> {
        self.sender.send(Message::ChatRoomLeave(ChatRoomPresence(chat, user)))?;
        Ok(())
    }
    pub fn recv(&mut self) -> BusResult<Option<Message>> {
        eprintln!("polling for messages");
        match self.receiver.try_recv() {
            Ok(message) => Ok(Some(message)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Lagged(n)) => Err(BusError::Lagged(n)),
            Err(TryRecvError::Closed) => Err(BusError::Closed),
        }
    }
    pub fn messages(&mut self) -> impl Stream<Item = BusResult<Message>> {
        let mut receiver = self.sender.subscribe();
        stream! {
            loop {
                let result = match receiver.recv().await {
                    Ok(message) => Ok(message),
                    Err(RecvError::Lagged(n)) => Err(BusError::Lagged(n)),
                    Err(RecvError::Closed) => Err(BusError::Closed),
                };
                yield result;
            }
        }
    }
}

#[derive(Debug, Clone, From, Into)]
pub struct Broadcast(pub Vec<u8>);

impl Into<ServerMessage> for Broadcast {
    fn into(self) -> ServerMessage {
        let Self(message) = self;
        ServerMessage {
            message,
            user_id: None,
            user_name: None,
        }
    }
}

impl Clone for Bus {
    fn clone(&self) -> Self {
        let Self { sender, .. } = self;
        let sender = sender.clone();
        let receiver = sender.subscribe();
        Self { sender, receiver }
    }
}

#[derive(Debug, Error)]
pub enum EventError {
    #[error(transparent)]
    Bus(#[from] BusError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}

pub struct ServerEvents<S> {
    frames: Frames<S>,
    bus: Bus,
}

type EventItem = Result<Message, EventError>;

impl <S: AsyncRead + Unpin> ServerEvents<S> {
    pub fn new(reader: S, bus: Bus) -> Self {
        Self {
            frames: Frames::new(reader),
            bus,
        }
    }
    pub fn events(self) -> impl Stream<Item = EventItem> {
        let Self { mut bus, frames } = self;
        let frames = frames.frames()
            .map_ok(|f| Message::TransactionReceived(f))
            .map_err(EventError::Protocol);
        let messages = bus.messages()
            .map_err(EventError::Bus);
        select(frames, messages)
    }
}
