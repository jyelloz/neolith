use crate::{
    protocol::{self as proto, ChatId, UserId},
    server::{
        Broadcast, ChatRoomCreationRequest, ChatRoomInvite, ChatRoomLeave, ChatRoomPresence,
        ChatRoomSubject, InstantMessage,
        bus::{Bus, Notification},
    },
};

use derive_more::{From, Into};
use std::collections::HashSet;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, watch};
use tracing::debug;

#[derive(Debug, Error)]
pub enum ChatError {
    #[error("execution error")]
    ExecutionError(#[from] oneshot::error::RecvError),
    #[error("service unavailable")]
    ServiceUnavailable,
}

impl<T> From<mpsc::error::SendError<T>> for ChatError {
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
        let id = u16::from(*id).wrapping_add(1);
        Self(id.into(), ChatRoom::default())
    }
}

impl std::hash::Hash for ChatRoomId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        u16::from(self.0).hash(state);
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
        self.users.iter().cloned().collect()
    }
    pub fn contains(&self, user: &UserId) -> bool {
        self.users.contains(user)
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
        self.rooms.take(&tester).unwrap_or(tester)
    }
    fn return_room(&mut self, room: ChatRoomId) {
        self.rooms.insert(room);
    }
    pub fn create(&mut self, users: Vec<UserId>) -> ChatId {
        let chat = self.next.0;
        // FIXME: Ensure next ID is unused
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
        self.rooms
            .get(&ChatRoomId(chat_id, ChatRoom::default()))
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
    pub fn leave_all(&mut self, user: UserId) -> Vec<ChatId> {
        let chats = self
            .rooms
            .iter()
            .filter(|ChatRoomId(_, room)| room.contains(&user))
            .map(|ChatRoomId(id, _)| id)
            .cloned()
            .collect::<Vec<_>>();
        for chat_id in &chats {
            let mut room = self.take_room(*chat_id);
            room.remove(user);
            self.return_room(room);
        }
        chats
    }
}

#[derive(Debug)]
enum Command {
    Create(ChatRoomCreationRequest, oneshot::Sender<ChatId>),
    SubjectUpdate(ChatRoomSubject, oneshot::Sender<()>),
    UserJoin(ChatRoomPresence, oneshot::Sender<()>),
    UserLeave(ChatRoomLeave, oneshot::Sender<()>),
    UserLeaveAll(UserId, oneshot::Sender<Vec<ChatId>>),
}

pub struct ChatUpdateProcessor {
    queue: mpsc::Receiver<Command>,
    chats: Chats,
    updates: watch::Sender<Chats>,
}

#[derive(Debug, Clone)]
pub struct ChatsService(mpsc::Sender<Command>, Bus);

impl ChatsService {
    pub fn new(bus: Bus) -> (Self, ChatUpdateProcessor) {
        let (tx, rx) = mpsc::channel(10);
        let service = Self(tx, bus);
        let process = ChatUpdateProcessor::new(rx);
        (service, process)
    }
    pub async fn create(&mut self, request: ChatRoomCreationRequest) -> Result<ChatId> {
        let (tx, rx) = oneshot::channel();
        let ChatRoomCreationRequest(_, users) = request.clone();
        self.send(Command::Create(request, tx)).await?;
        let id = rx.await?;
        for user in users {
            self.publish(Notification::ChatRoomInvite((id, user).into()));
        }
        Ok(id)
    }
    pub async fn invite(&mut self, request: ChatRoomInvite) -> Result<()> {
        self.publish(Notification::ChatRoomInvite(request));
        Ok(())
    }
    pub async fn join(&mut self, request: ChatRoomPresence) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.send(Command::UserJoin(request.clone(), tx)).await?;
        rx.await?;
        self.publish(Notification::ChatRoomJoin(request));
        Ok(())
    }
    pub async fn leave(&mut self, request: ChatRoomLeave) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.send(Command::UserLeave(request.clone(), tx)).await?;
        rx.await?;
        self.publish(Notification::ChatRoomLeave(request));
        Ok(())
    }
    pub async fn change_subject(&mut self, request: ChatRoomSubject) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.send(Command::SubjectUpdate(request.clone(), tx))
            .await?;
        rx.await?;
        self.publish(Notification::ChatRoomSubjectUpdate(request));
        Ok(())
    }
    pub async fn chat(&mut self, chat: proto::ChatMessage) -> Result<()> {
        self.publish(chat.into());
        Ok(())
    }
    pub async fn broadcast(&mut self, message: Broadcast) -> Result<()> {
        self.publish(message.into());
        Ok(())
    }
    pub async fn instant_message(&mut self, message: InstantMessage) -> Result<()> {
        self.publish(message.into());
        Ok(())
    }
    pub async fn leave_all(&mut self, request: UserId) -> Result<Vec<ChatId>> {
        let (tx, rx) = oneshot::channel();
        self.send(Command::UserLeaveAll(request, tx)).await?;
        let chats = rx.await?;
        Ok(chats)
    }
    async fn send(&mut self, command: Command) -> Result<()> {
        let Self(tx, _) = self;
        tx.send(command).await?;
        Ok(())
    }
    fn publish(&mut self, notification: Notification) {
        let Self(_, bus) = self;
        bus.publish(notification);
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
    #[tracing::instrument(name = "ChatUpdateProcessor", skip(self))]
    pub async fn run(self) -> Result<()> {
        let Self {
            mut chats,
            mut queue,
            updates,
        } = self;
        while let Some(command) = queue.recv().await {
            debug!("handling update: {:?}", &command);
            match command {
                Command::Create(request, tx) => {
                    let ChatRoomCreationRequest(creator, ..) = request;
                    let id = chats.create(vec![creator]);
                    if tx.send(id).is_err() {
                        Err(ChatError::ServiceUnavailable)?;
                    }
                }
                Command::UserJoin(presence, tx) => {
                    let ChatRoomPresence(chat, user) = presence;
                    chats.join(chat, user.into());
                    if tx.send(()).is_err() {
                        Err(ChatError::ServiceUnavailable)?;
                    }
                }
                Command::UserLeave(leave, tx) => {
                    let ChatRoomLeave(chat, user) = leave;
                    chats.leave(chat, user);
                    if tx.send(()).is_err() {
                        Err(ChatError::ServiceUnavailable)?;
                    }
                }
                Command::SubjectUpdate(presence, tx) => {
                    let ChatRoomSubject(chat, subject) = presence;
                    chats.set_subject(chat, subject);
                    if tx.send(()).is_err() {
                        Err(ChatError::ServiceUnavailable)?;
                    }
                }
                Command::UserLeaveAll(user, tx) => {
                    let chats = chats.leave_all(user);
                    if tx.send(chats).is_err() {
                        Err(ChatError::ServiceUnavailable)?;
                    }
                }
            }
            if updates.send(chats.clone()).is_err() {
                debug!("ChatUpdateProcessor: shutting down");
                break;
            }
        }
        Ok(())
    }
    pub fn subscribe(&self) -> watch::Receiver<Chats> {
        self.updates.subscribe()
    }
}

pub fn format_chat(username: &proto::Nickname, message: &[u8]) -> Vec<u8> {
    [
        b"\r ".as_slice(),
        username.as_slice(),
        b": ".as_slice(),
        message,
    ]
    .concat()
}
