use crate::protocol::{
    UserId,
    UserNameWithInfo,
};

#[derive(Debug, Clone)]
pub struct Users(Vec<UserNameWithInfo>, usize);

impl Users {
    pub fn new() -> Self {
        Self(vec![], 1)
    }
    pub fn add(&mut self, user: &mut UserNameWithInfo) -> UserId {
        let Self(users, top_id) = self;
        let user_id = (*top_id as i16).into();
        user.user_id = user_id;
        *top_id += 1;
        users.push(user.clone());
        user_id
    }
    pub fn update(&mut self, user: &UserNameWithInfo) -> Option<UserId> {
        let Self(users, ..) = self;
        if let Some(u) = users.iter_mut().find(|u| u.user_id == user.user_id) {
            *u = user.clone();
            Some(user.user_id)
        } else {
            None
        }
    }
    pub fn remove(&mut self, user: &UserNameWithInfo) {
        let Self(users, ..) = self;
        users.retain(|u| u.user_id != user.user_id);
    }
    pub fn borrow(&self) -> &[UserNameWithInfo] {
        &self.0
    }
}
