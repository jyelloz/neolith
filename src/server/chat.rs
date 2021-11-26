use crate::{
    protocol::{ChatId, UserId},
    server::{
        ChatRoomCreationRequest,
        ChatRoomSubject,
        ChatRoomPresence,
    },
};

use thiserror::Error;

use derive_more::{From, Into};

use std::collections::HashSet;
use tokio::sync::{mpsc, oneshot, watch};

#[derive(Debug, Error)]
pub enum ChatError {
    #[error("execution error")]
    ExecutionError(#[from] oneshot::error::RecvError),
    #[error("service unavailable")]
    ServiceUnavailable,
}

impl <T> From<mpsc::error::SendError<T>> for ChatError {
    fn from(_: mpsc::error::SendError<T>) -> Self {
        Self::ServiceUnavailable
    }
}

type Result<T> = ::core::result::Result<T, ChatError>;

#[derive(Debug, Default, Clone, From, Into, Eq)]
pub struct ChatRoomId(ChatId, ChatRoom);

impl ChatRoomId {
    pub fn next(&self) -> Self {
        let Self(id, _) = self;
        let id: i32 = i32::from(*id) + 1i32;
        Self(id.into(), Default::default())
    }
}

impl std::hash::Hash for ChatRoomId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        i32::from(self.0).hash(state);
    }
}

impl std::cmp::PartialEq for ChatRoomId {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl ChatRoomId {
    fn add(&mut self, user: UserId) {
        let Self(_, room) = self;
        room.add(user)
    }
    fn remove(&mut self, user: UserId) {
        let Self(_, room) = self;
        room.remove(user)
    }
}

#[derive(Debug, Default, Clone, From, Into, PartialEq, Eq)]
pub struct ChatRoom {
    pub subject: Option<Vec<u8>>,
    users: HashSet<UserId>,
}

impl ChatRoom {
    pub fn add(&mut self, user: UserId) {
        self.users.insert(user);
    }
    pub fn remove(&mut self, user: UserId) {
        self.users.remove(&user);
    }
    pub fn users(&self) -> Vec<UserId> {
        self.users.iter()
            .cloned()
            .collect()
    }
}

#[derive(Debug, Default, Clone)]
pub struct Chats {
    rooms: HashSet<ChatRoomId>,
    next: ChatRoomId,
}

impl Chats {
    pub fn new() -> Self {
        Self::default()
    }
    fn take_room(&mut self, chat_id: ChatId) -> ChatRoomId {
        let tester = ChatRoomId(chat_id, ChatRoom::default());
        self.rooms.take(&tester)
            .unwrap_or(tester)
    }
    fn return_room(&mut self, room: ChatRoomId) {
        self.rooms.insert(room);
    }
    pub fn create(&mut self, users: Vec<UserId>) -> ChatId {
        let chat = self.next.0;
        self.next = self.next.next();
        for user in users {
            self.join(chat, user);
        }
        chat
    }
    pub fn join(&mut self, chat_id: ChatId, user: UserId) {
        let mut room = self.take_room(chat_id);
        room.add(user);
        self.return_room(room);
    }
    pub fn leave(&mut self, chat_id: ChatId, user: UserId) {
        let mut room = self.take_room(chat_id);
        room.remove(user);
        self.return_room(room);
    }
    pub fn room(&self, chat_id: ChatId) -> Option<&ChatRoom> {
        self.rooms.get(&ChatRoomId(chat_id, ChatRoom::default()))
            .map(|room| &room.1)
    }
    pub fn set_subject(&mut self, chat_id: ChatId, subject: Vec<u8>) {
        let mut room = self.take_room(chat_id);
        {
            let ChatRoomId(_, room) = &mut room;
            room.subject.replace(subject);
        }
        self.return_room(room)
    }
}

#[derive(Debug)]
enum Command {
    // Chat(Chat),
    ChatRoomCreate(ChatRoomCreationRequest, oneshot::Sender<ChatId>),
    ChatRoomSubjectUpdate(ChatRoomSubject, oneshot::Sender<()>),
    ChatRoomUserJoin(ChatRoomPresence, oneshot::Sender<()>),
    ChatRoomUserUpdate(ChatRoomPresence, oneshot::Sender<()>),
    ChatRoomUserLeave(ChatRoomPresence, oneshot::Sender<()>),
}

pub struct ChatUpdateProcessor {
    queue: mpsc::Receiver<Command>,
    chats: Chats,
    updates: watch::Sender<Chats>,
}

#[derive(Debug, Clone)]
pub struct ChatsList(mpsc::Sender<Command>);

impl ChatsList {
    pub fn new() -> (Self, ChatUpdateProcessor) {
        let (tx, rx) = mpsc::channel(10);
        let list = Self(tx);
        let proc = ChatUpdateProcessor::new(rx);
        (list, proc)
    }
    pub async fn create(
        &mut self,
        request: ChatRoomCreationRequest,
    ) -> Result<ChatId> {
        let (tx, rx) = oneshot::channel();
        self.0.send(Command::ChatRoomCreate(request, tx)).await?;
        let id = rx.await?;
        Ok(id)
    }
    pub async fn join(
        &mut self,
        request: ChatRoomPresence,
    ) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.0.send(Command::ChatRoomUserJoin(request, tx)).await?;
        rx.await?;
        Ok(())
    }
    pub async fn update(
        &mut self,
        request: ChatRoomPresence,
    ) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.0.send(Command::ChatRoomUserUpdate(request, tx)).await?;
        rx.await?;
        Ok(())
    }
    pub async fn leave(
        &mut self,
        request: ChatRoomPresence,
    ) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.0.send(Command::ChatRoomUserLeave(request, tx)).await?;
        rx.await?;
        Ok(())
    }
    pub async fn change_subject(
        &mut self,
        request: ChatRoomSubject,
    ) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.0.send(Command::ChatRoomSubjectUpdate(request, tx)).await?;
        rx.await?;
        Ok(())
    }
}

impl ChatUpdateProcessor {
    fn new(queue: mpsc::Receiver<Command>) -> Self {
        let chats = Chats::new();
        let (updates, _) = watch::channel(chats.clone());
        Self {
            queue,
            chats,
            updates,
        }
    }
    pub async fn run(self) -> Result<()> {
        let Self { mut chats, mut queue, updates } = self;
        while let Some(command) = queue.recv().await {
            eprintln!("handling update: {:?}", &command);
            match command {
                Command::ChatRoomCreate(users, tx) => {
                    let id = chats.create(users.clone().into());
                    if tx.send(id).is_err() {
                        Err(ChatError::ServiceUnavailable)?;
                    }
                }
                Command::ChatRoomUserJoin(presence, tx) => {
                    let ChatRoomPresence(chat, user) = presence;
                    chats.join(chat, user.into());
                    if tx.send(()).is_err() {
                        Err(ChatError::ServiceUnavailable)?;
                    }
                }
                Command::ChatRoomUserUpdate(_, tx) => {
                    if tx.send(()).is_err() {
                        Err(ChatError::ServiceUnavailable)?;
                    }
                }
                Command::ChatRoomUserLeave(presence, tx) => {
                    let ChatRoomPresence(chat, user) = presence;
                    chats.leave(chat, user.into());
                    if tx.send(()).is_err() {
                        Err(ChatError::ServiceUnavailable)?;
                    }
                }
                Command::ChatRoomSubjectUpdate(presence, tx) => {
                    let ChatRoomSubject(chat, subject) = presence;
                    chats.set_subject(chat, subject);
                    if tx.send(()).is_err() {
                        Err(ChatError::ServiceUnavailable)?;
                    }
                }
            }
            if updates.send(chats.clone()).is_err() {
                eprintln!("ChatUpdateProcessor: shutting down");
                break;
            }
        }
        Ok(())
    }
    pub fn subscribe(&self) -> watch::Receiver<Chats> {
        self.updates.subscribe()
    }
}
