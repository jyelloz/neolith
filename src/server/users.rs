use crate::protocol::{self as proto, UserId, UserNameWithInfo};

use derive_more::{From, Into};
use thiserror::Error;

use std::collections::HashSet;

use tokio::sync::{mpsc, oneshot, watch};

use tracing::debug;

use super::bus::{Bus, Notification};

#[derive(Debug, Error)]
pub enum UsersError {
    #[error("execution error")]
    ExecutionError(#[from] oneshot::error::RecvError),
    #[error("service unavailable")]
    ServiceUnavailable,
}

impl<T> From<mpsc::error::SendError<T>> for UsersError {
    fn from(_: mpsc::error::SendError<T>) -> Self {
        Self::ServiceUnavailable
    }
}

type Result<T> = ::core::result::Result<T, UsersError>;

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
            username: vec![].into(),
            username_len: 0,
        };
        users.get(&fake_user.into()).map(|u| &u.0)
    }
    pub fn to_vec(&self) -> Vec<UserNameWithInfo> {
        self.0.iter().cloned().map(User::into).collect()
    }
}

impl Default for Users {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
enum Command {
    Connect(UserNameWithInfo, oneshot::Sender<UserId>),
    Update(UserNameWithInfo, oneshot::Sender<()>),
    Disconnect(UserNameWithInfo, oneshot::Sender<()>),
}

#[derive(Debug, Clone, From)]
pub struct UsersService(mpsc::Sender<Command>, Bus);

impl UsersService {
    pub fn new(bus: Bus) -> (Self, UserUpdateProcessor) {
        let (tx, rx) = mpsc::channel(10);
        let service = Self(tx, bus);
        let process = UserUpdateProcessor::new(rx);
        (service, process)
    }
    pub async fn add(&mut self, mut user: UserNameWithInfo) -> Result<UserId> {
        let (tx, rx) = oneshot::channel();
        let command = Command::Connect(user.clone(), tx);
        let Self(tx, bus) = self;
        tx.send(command).await?;
        let id = rx.await?;
        user.user_id = id;
        let notification = Notification::UserConnect(user.into());
        bus.publish(notification);
        Ok(id)
    }
    pub async fn update(&mut self, user: UserNameWithInfo) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        let notification = Notification::UserUpdate(user.clone().into());
        let command = Command::Update(user, tx);
        let Self(tx, bus) = self;
        tx.send(command).await?;
        rx.await?;
        bus.publish(notification);
        Ok(())
    }
    pub async fn delete(&mut self, user: UserNameWithInfo) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        let notification = Notification::UserDisconnect(user.clone().into());
        let command = Command::Disconnect(user, tx);
        let Self(tx, bus) = self;
        tx.send(command).await?;
        rx.await?;
        bus.publish(notification);
        Ok(())
    }
}

pub struct UserUpdateProcessor {
    queue: mpsc::Receiver<Command>,
    users: Users,
    updates: watch::Sender<Users>,
}

impl UserUpdateProcessor {
    fn new(queue: mpsc::Receiver<Command>) -> Self {
        let users = Users::new();
        let (updates, _rx) = watch::channel(users.clone());
        Self {
            queue,
            users,
            updates,
        }
    }
    #[tracing::instrument(name = "UserUpdateProcessor", skip(self))]
    pub async fn run(self) -> Result<()> {
        let Self {
            mut users,
            mut queue,
            updates,
        } = self;
        while let Some(command) = queue.recv().await {
            debug!("handling update: {:?}", &command);
            match command {
                Command::Connect(mut user, tx) => {
                    let id = users.add(&mut user);
                    tx.send(id).ok();
                }
                Command::Update(user, tx) => {
                    users.update(&user);
                    tx.send(()).ok();
                }
                Command::Disconnect(user, tx) => {
                    users.remove(&user);
                    tx.send(()).ok();
                }
            }
            if updates.send(users.clone()).is_err() {
                debug!("UserUpdateProcessor: shutting down");
                break;
            }
        }
        Ok(())
    }
    pub fn subscribe(&self) -> watch::Receiver<Users> {
        self.updates.subscribe()
    }
}
