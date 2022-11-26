use tokio::io::AsyncRead;
use futures::stream::{
    TryStreamExt as _,
    StreamExt as _,
    Stream,
    select,
};
use tokio::sync::broadcast::error::{SendError, RecvError};

use derive_more::{From, Into};

use thiserror::Error;

pub mod application;
pub mod bus;
pub mod files;
pub mod users;
pub mod chat;
pub mod news;
pub mod transaction_stream;
pub mod transfers;

use self::bus::{
    Notification,
    Notifications,
};

use crate::protocol::{
    ChatId,
    ChatMessage,
    Message,
    NotifyNewsMessage,
    ProtocolError,
    ServerMessage,
    TransactionFrame,
    UserId,
    UserNameWithInfo,
};

use transaction_stream::Frames;

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
pub struct ChatRoomSubject(pub ChatId, pub Vec<u8>);

#[derive(Debug, Clone, From, Into)]
pub struct ChatRoomCreationRequest(pub Vec<UserId>);

#[derive(Debug, Clone, From, Into)]
pub struct ChatRoomPresence(pub ChatId, pub User);

#[derive(Debug, Clone, From, Into)]
pub struct ChatRoomInvite(pub ChatId, pub UserId);

#[derive(Debug, Clone)]
pub struct InstantMessage {
    pub from: User,
    pub to: User,
    pub message: Vec<u8>,
}

pub type BusResult<T> = Result<T, BusError>;

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

#[derive(Debug, Clone, From, Into)]
pub struct Article(pub Vec<u8>);

impl Into<NotifyNewsMessage> for Article {
    fn into(self) -> NotifyNewsMessage {
        let Self(mut message) = self;
        message.extend_from_slice(news::SEPARATOR.as_bytes());
        let message = Message::from(message);
        NotifyNewsMessage::from(message)
    }
}

pub enum Event {
    Notification(Notification),
    Frame(TransactionFrame),
}

#[derive(Debug, Error)]
pub enum EventError {
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}

pub struct ServerEvents<S> {
    frames: Frames<S>,
    notifications: Notifications,
}

type EventItem = Result<Event, EventError>;

impl <S: AsyncRead + Unpin> ServerEvents<S> {
    pub fn new(reader: S, notifications: Notifications) -> Self {
        Self {
            frames: Frames::new(reader),
            notifications,
        }
    }
    fn notifications(notifications: Notifications) -> impl Stream<Item = EventItem> {
        notifications.incoming()
            .map(|n| Ok(Event::Notification(n)))
    }
    fn frames<F: AsyncRead + Unpin>(frames: Frames<F>) -> impl Stream<Item = EventItem> {
        frames.frames()
            .map_ok(|f| Event::Frame(f))
            .map_err(ProtocolError::into)
    }
    pub fn events(self) -> impl Stream<Item = EventItem> {
        let Self { frames, notifications } = self;
        let frames = Self::frames(frames);
        let notifications = Self::notifications(notifications);
        select(frames, notifications)
    }
}
