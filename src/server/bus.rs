use tokio::sync::broadcast;

use derive_more::{From, Into};

use super::{
    Chat,
    ChatMessage,
    ChatRoomCreationRequest,
    ChatRoomInvite,
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
    ChatRoomSubjectUpdate(ChatRoomSubject),
    ChatRoomInvite(ChatRoomInvite),
    ChatRoomJoin(ChatRoomPresence),
    ChatRoomLeave(ChatRoomPresence),
    Broadcast(Broadcast),
    InstantMessage(InstantMessage),
    UserConnect(User),
    UserUpdate(User),
    UserDisconnect(User),
}

impl From<ChatMessage> for Notification {
    fn from(message: ChatMessage) -> Self {
        Self::Chat(message)
    }
}

/// A publish-subscribe node between connected peers and the backing state
/// components of the server.
///
/// It provices capabilities for users broadcasting chat messages and the server
/// notifying clients of user presence updates.
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
