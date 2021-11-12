use crate::protocol::{
    UserId,
    UserNameWithInfo,
};

use derive_more::{From, Into};

use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct Users(HashSet<User>, i16);

#[derive(Debug, Clone, From, Into, Eq)]
struct User(UserNameWithInfo);

impl std::hash::Hash for User {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        i16::from(self.0.user_id).hash(state);
    }
}

impl std::cmp::PartialEq for User {
    fn eq(&self, other: &Self) -> bool {
        self.0.user_id == other.0.user_id
    }
}

impl Users {
    pub fn new() -> Self {
        Self(HashSet::new(), 1)
    }
    pub fn add(&mut self, user: &mut UserNameWithInfo) -> UserId {
        let Self(users, top_id) = self;
        let user_id = (*top_id).into();
        user.user_id = user_id;
        *top_id += 1;
        users.insert(user.clone().into());
        user_id
    }
    pub fn update(&mut self, user: &UserNameWithInfo) -> Option<UserId> {
        let user_id = user.user_id;
        let Self(users, ..) = self;
        let user = User::from(user.clone());
        users.replace(user)?;
        Some(user_id)
    }
    pub fn remove(&mut self, user: &UserNameWithInfo) {
        let Self(users, ..) = self;
        let user = User::from(user.clone());
        users.remove(&user);
    }
    pub fn find(&self, id: UserId) -> Option<&UserNameWithInfo> {
        let Self(users, ..) = self;
        let fake_user = UserNameWithInfo {
            user_id: id,
            user_flags: 0.into(),
            icon_id: 0.into(),
            username: b"".to_vec().into(),
        };
        users.get(&fake_user.into())
            .map(|u| &u.0)
    }
    pub fn to_vec(&self) -> Vec<UserNameWithInfo> {
        self.0.iter()
            .cloned()
            .map(User::into)
            .collect()
    }
}
