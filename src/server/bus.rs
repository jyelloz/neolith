use tokio::sync::broadcast;

use derive_more::{From, Into};

use super::{
    Chat,
    ChatMessage,
    ChatRoomCreationRequest,
    ChatRoomPresence,
    ChatRoomSubject,
    User,
    Broadcast,
    InstantMessage,
};

#[derive(Debug, Clone)]
pub enum Command {
    Chat(Chat),
    ChatRoomCreate(ChatRoomCreationRequest),
    ChatRoomSubjectUpdate(ChatRoomSubject),
    ChatRoomUserJoin(ChatRoomPresence),
    ChatRoomUserUpdate(ChatRoomPresence),
    ChatRoomUserLeave(ChatRoomPresence),
    Broadcast(Broadcast),
    InstantMessage(InstantMessage),
    UserConnect(User),
    UserUpdate(User),
    UserDisconnect(User),
}

#[derive(Debug, Clone)]
pub enum Notification {
    Empty,
    Chat(ChatMessage),
    ChatRoomJoin(ChatRoomPresence),
    ChatRoomLeave(ChatRoomPresence),
    Broadcast(Broadcast),
    InstantMessage(InstantMessage),
    UserConnect(User),
    UserUpdate(User),
    UserDisconnect(User),
}

/// A middleware between the network server's connections and its backing state.
///
/// It processes Commands using a set of configured filters which may mutate
/// internal state based on the Command and also may react by submitting a
/// Notification which is broadcast across the system, allowing connections to
/// notify the client or some other internal component to begin a
/// synchronization action.
#[derive(Debug, Clone)]
pub struct Bus {
    tx: broadcast::Sender<Notification>,
}

impl Bus {
    pub fn new() -> Self {
        Self::new_with_buffer(10)
    }
    pub fn new_with_buffer(buffer: usize) -> Self {
        let (tx, _) = broadcast::channel(buffer);
        Self { tx }
    }
    pub fn publish(&self, notification: Notification) {
        self.tx.send(notification).ok();
    }
    pub fn subscribe(&self) -> Notifications {
        self.tx.subscribe().into()
    }
}

#[derive(Debug, From, Into)]
pub struct Notifications(broadcast::Receiver<Notification>);

impl Notifications {
    pub fn incoming(self) -> impl futures::stream::Stream<Item = Notification> {
        let Self(mut notifications) = self;
        async_stream::stream! {
            while let Ok(notification) = notifications.recv().await {
                yield notification;
            }
        }
    }
}
