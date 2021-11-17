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

#[derive(Debug, Clone, From, Into, PartialEq, Eq)]
pub struct ChatRoom(HashSet<User>);

impl ChatRoom {
    pub fn new() -> Self {
        Self(HashSet::new())
    }
    pub fn add(&mut self, user: &UserNameWithInfo) {
        self.0.insert(user.clone().into());
    }
    pub fn remove(&mut self, user: &UserNameWithInfo) {
        self.0.remove(&user.clone().into());
    }
}

pub struct Chats(HashSet<ChatRoomId>);

impl Chats {
    pub fn new() -> Self {
        Self(HashSet::new())
    }
    pub fn join(&mut self, user: &UserNameWithInfo, chat_id: ChatId) {
    }
    pub fn leave(&mut self, user: &UserNameWithInfo, chat_id: ChatId) {
    }
    pub fn room(&self, chat_id: ChatId) -> Vec<UserNameWithInfo> {
        self.0.get(&ChatRoomId(chat_id, ChatRoom::new()))
            .into_iter()
            .flat_map(|id_and_room| id_and_room.1.0.clone().into_iter())
            .map(User::into)
            .collect()
    }
}
