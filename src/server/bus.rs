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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub enum Notification {
}

pub trait Filter {
    fn process(
        &mut self,
        command: &Command,
        notifications: bc::Sender<Notification>,
    );
}

type Filters = Vec<Box<dyn Filter + Send>>;

/// A middleware between the network server's connections and its backing state.
///
/// It processes Commands using a set of configured filters which may mutate
/// internal state based on the Command and also may react by submitting a
/// Notification which is broadcast across the system, allowing connections to
/// notify the client or some other internal component to begin a
/// synchronization action.
pub struct Bus {
    commands_tx: mpsc::Sender<Command>,
    commands: mpsc::Receiver<Command>,
    filters: Filters,
    notifications: bc::Sender<Notification>,
}

impl Bus {
    pub fn new() -> Self {
        Self::new_with_filters(vec![])
    }
    pub fn new_with_filters(filters: Filters) -> Self {
        Self::new_with_buffer_and_filters(10, filters)
    }
    pub fn new_with_buffer(buffer: usize) -> Self {
        Self::new_with_buffer_and_filters(buffer, vec![])
    }
    pub fn new_with_buffer_and_filters(buffer: usize, filters: Filters) -> Self {
        let (commands_tx, commands) = mpsc::channel(buffer);
        let (notifications, _) = bc::channel(buffer);
        Self {
            commands_tx,
            commands,
            filters,
            notifications,
        }
    }
    fn process(command: Command, filters: &mut Filters, notifications: &mut bc::Sender<Notification>) {
        for filter in filters {
            filter.process(&command, notifications.clone());
        }
    }
    /// Represents a long-running task which will process all incoming commands.
    /// This consumes the underlying Bus.
    pub async fn run(self) {
        let Self {
            commands_tx,
            mut commands,
            mut notifications,
            mut filters,
        } = self;
        drop(commands_tx);
        while let Some(command) = commands.recv().await {
            Self::process(command, &mut filters, &mut notifications);
        }
        eprintln!("Bus: shutting down");
    }
    pub fn add_filter(&mut self, filter: Box<dyn Filter + Send>) {
        self.filters.push(filter)
    }
    pub fn command_publisher(&self) -> mpsc::Sender<Command> {
        self.commands_tx.clone()
    }
    pub fn notification_subscriber(&self) -> bc::Receiver<Notification> {
        self.notifications.subscribe()
    }
}
