use derive_more::From;

use crate::protocol as proto;

#[derive(Debug, From)]
pub enum ClientResponse {
    RejectChatInvite(proto::RejectChatInvite),
}
