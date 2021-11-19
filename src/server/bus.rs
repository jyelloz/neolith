use tokio::sync::{
    mpsc,
    broadcast as bc,
};

use super::{
    Chat,
    ChatId,
    User,
    Broadcast,
    InstantMessage,
};

/// A middleware between the network server's connections and its backing state.
///
/// It processes Commands using a set of configured filters which may mutate
/// internal state based on the Command and also may react by submitting a
/// Notification which is broadcast across the system, allowing connections to
/// notify the client or some other internal component to begin a
/// synchronization action.
pub struct Bus {
    commands: mpsc::Receiver<Command>,
    filters: Vec<Box<dyn Filter + Send>>,
    notifications: bc::Sender<Notification>,
}

pub enum Command {
    Chat(Chat),
    ChatRoomSubjectUpdate(ChatId, Vec<u8>),
    ChatRoomUserJoin(ChatId, User),
    ChatRoomUserUpdate(ChatId, User),
    ChatRoomUserLeave(ChatId, User),
    ChatRoomChat(ChatId, User, Vec<u8>),
    Broadcast(Broadcast),
    InstantMessage(InstantMessage),
    UserConnect(User),
    UserUpdate(User),
    UserDisconnect(User),
}

pub enum Notification {
}

pub trait Filter {
    fn process(
        &mut self,
        command: &Command,
        notifications: bc::Sender<Notification>,
    );
}

impl Bus {
    pub fn new(
        commands: mpsc::Receiver<Command>,
        filters: Vec<Box<dyn Filter + Send>>,
        notifications: bc::Sender<Notification>,
    ) -> Self {
        Self {
            commands,
            filters,
            notifications,
        }
    }
    fn process(&mut self, command: Command) {
        let Self { filters, notifications, .. } = self;
        let filters = filters.as_mut_slice();
        for filter in filters {
            filter.process(&command, notifications.clone());
        }
    }
    async fn next_command(&mut self) -> Option<Command> {
        self.commands.recv().await
    }
    pub async fn run(mut self) {
        while let Some(command) = self.next_command().await {
            self.process(command);
        }
        eprintln!("Bus: shutting down");
    }
}
