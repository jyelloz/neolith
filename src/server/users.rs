use crate::protocol::{
    UserId,
    UserNameWithInfo,
};

#[derive(Debug, Clone)]
pub struct Users(Vec<UserNameWithInfo>, usize);

impl Users {
    pub fn new() -> Self {
        Self(vec![], 0)
    }
    pub fn add(&mut self, user: &mut UserNameWithInfo) -> UserId {
        let Self(users, top_id) = self;
        let user_id = (*top_id as i16).into();
        user.user_id = user_id;
        *top_id += 1;
        users.push(user.clone());
        user_id
    }
    pub fn remove(&mut self, user: &UserNameWithInfo) {
        let Self(users, ..) = self;
        users.retain(|u| u.user_id != user.user_id);
    }
    pub fn borrow(&self) -> &[UserNameWithInfo] {
        &self.0
    }
}
