use crate::protocol as proto;
use derive_more::From;

#[derive(Debug, From)]
pub enum ServerResponse {
    LoginReply,
    GetUserNameListReply(proto::GetUserNameListReply),
    GetClientInfoTextReply(proto::GetClientInfoTextReply),
    GetMessagesReply(proto::GetMessagesReply),
    PostNewsReply,
    GetFileNameListReply(proto::GetFileNameListReply),
    GetFileInfoReply(proto::GetFileInfoReply),
    SetFileInfoReply(proto::SetFileInfoReply),
    DownloadFileReply(proto::DownloadFileReply),
    UploadFileReply(proto::UploadFileReply),
    DeleteFileReply(proto::DeleteFileReply),
    MoveFileReply(proto::MoveFileReply),
    GetUserReply(proto::GetUserReply),
    SendInstantMessageReply,
    JoinChatReply(proto::JoinChatReply),
    InviteToNewChatReply(proto::InviteToNewChatReply),
    NewFolderReply,
    SendBroadcastReply,
    SetUserReply,
    NewUserReply,
    DeleteUserReply,
    Rejected(Option<String>),
}

impl ServerResponse {
    fn reject(message: Option<String>) -> proto::TransactionFrame {
        let mut frame = proto::TransactionFrame::empty(proto::TransactionType::Error);
        frame.header.error_code = 1u32.into();
        if let Some(reason) = message {
            frame
                .body
                .parameters
                .push(proto::Parameter::new_error(reason));
        }
        frame
    }
}

impl From<ServerResponse> for proto::TransactionFrame {
    fn from(val: ServerResponse) -> Self {
        match val {
            ServerResponse::LoginReply => proto::GenericReply.into(),
            ServerResponse::GetUserNameListReply(reply) => reply.into(),
            ServerResponse::GetMessagesReply(reply) => reply.into(),
            ServerResponse::PostNewsReply => proto::GenericReply.into(),
            ServerResponse::GetFileNameListReply(reply) => reply.into(),
            ServerResponse::GetFileInfoReply(reply) => reply.into(),
            ServerResponse::SetFileInfoReply(reply) => reply.into(),
            ServerResponse::GetClientInfoTextReply(reply) => reply.into(),
            ServerResponse::DownloadFileReply(reply) => reply.into(),
            ServerResponse::UploadFileReply(reply) => reply.into(),
            ServerResponse::DeleteFileReply(reply) => reply.into(),
            ServerResponse::MoveFileReply(reply) => reply.into(),
            ServerResponse::NewFolderReply => proto::GenericReply.into(),
            ServerResponse::GetUserReply(reply) => reply.into(),
            ServerResponse::Rejected(message) => ServerResponse::reject(message),
            ServerResponse::SetUserReply => proto::GenericReply.into(),
            ServerResponse::NewUserReply => proto::GenericReply.into(),
            ServerResponse::DeleteUserReply => proto::GenericReply.into(),
            ServerResponse::SendBroadcastReply => proto::GenericReply.into(),
            ServerResponse::SendInstantMessageReply => proto::GenericReply.into(),
            ServerResponse::JoinChatReply(reply) => reply.into(),
            ServerResponse::InviteToNewChatReply(reply) => reply.into(),
        }
    }
}
