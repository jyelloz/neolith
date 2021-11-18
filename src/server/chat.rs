use super::users::User;

use crate::protocol::{ChatId, UserNameWithInfo};

use derive_more::{From, Into};

use std::collections::HashSet;

#[derive(Debug, Clone, From, Into, Eq)]
pub struct ChatRoomId(ChatId, ChatRoom);

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
    fn add(&mut self, user: &UserNameWithInfo) {
        let Self(_, room) = self;
        room.add(user)
    }
    fn remove(&mut self, user: &UserNameWithInfo) {
        let Self(_, room) = self;
        room.remove(user)
    }
}

#[derive(Debug, Default, Clone, From, Into, PartialEq, Eq)]
pub struct ChatRoom{
    pub subject: Option<Vec<u8>>,
    users: HashSet<User>,
}

impl ChatRoom {
    pub fn add(&mut self, user: &UserNameWithInfo) {
        self.users.insert(user.clone().into());
    }
    pub fn remove(&mut self, user: &UserNameWithInfo) {
        self.users.remove(&user.clone().into());
    }
}

#[derive(Debug, Default)]
pub struct Chats(HashSet<ChatRoomId>);

impl Chats {
    pub fn new() -> Self {
        Self::default()
    }
    fn take_room(&mut self, chat_id: ChatId) -> ChatRoomId {
        let tester = ChatRoomId(chat_id, ChatRoom::default());
        self.0.take(&tester)
            .unwrap_or(tester)
    }
    fn return_room(&mut self, room: ChatRoomId) {
        self.0.insert(room);
    }
    pub fn join(&mut self, chat_id: ChatId, user: &UserNameWithInfo) {
        let mut room = self.take_room(chat_id);
        room.add(user);
        self.return_room(room);
    }
    pub fn leave(&mut self, chat_id: ChatId, user: &UserNameWithInfo) {
        let mut room = self.take_room(chat_id);
        room.remove(user);
        self.return_room(room);
    }
    pub fn room(&self, chat_id: ChatId) -> Option<&ChatRoom> {
        self.0.get(&ChatRoomId(chat_id, ChatRoom::default()))
            .map(|room| &room.1)
    }
}
