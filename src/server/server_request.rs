use crate::{protocol as proto, server};

#[derive(Debug, Clone)]
pub enum ServerRequest {
    Empty,
    Chat(proto::ChatMessage),
    ChatRoomSubjectUpdate(server::ChatRoomSubject),
    ChatRoomInvite(server::ChatRoomInvite),
    ChatRoomJoin(server::ChatRoomPresence),
    ChatRoomLeave(server::ChatRoomLeave),
    Broadcast(server::Broadcast),
    News(server::Article),
    InstantMessage(server::InstantMessage),
    UserConnect(server::User),
    UserUpdate(server::User),
    UserDisconnect(server::User),
}
