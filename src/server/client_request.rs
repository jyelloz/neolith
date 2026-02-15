use crate::protocol as proto;
use derive_more::From;

#[derive(Debug, From)]
pub enum ClientRequest {
    Login(proto::LoginRequest),
    GetMessages(proto::GetMessages),
    PostNews(proto::PostNews),
    GetFileNameList(proto::GetFileNameList),
    GetFileInfo(proto::GetFileInfo),
    SetFileInfo(proto::SetFileInfo),
    GetUserNameList(proto::GetUserNameList),
    GetClientInfoText(proto::GetClientInfoText),
    SetClientUserInfo(proto::SetClientUserInfo),
    DisconnectUser(proto::DisconnectUser),
    SendChat(proto::SendChat),
    SendInstantMessage(proto::SendInstantMessage),
    InviteToNewChat(proto::InviteToNewChat),
    InviteToChat(proto::InviteToChat),
    JoinChat(proto::JoinChat),
    LeaveChat(proto::LeaveChat),
    RejectChatInvite(proto::RejectChatInvite),
    SetChatSubject(proto::SetChatSubject),
    DownloadFile(proto::DownloadFile),
    UploadFile(proto::UploadFile),
    DeleteFile(proto::DeleteFile),
    MoveFile(proto::MoveFile),
    NewFolder(proto::NewFolder),
    MakeFileAlias(proto::MakeFileAlias),
    NewUser(proto::NewUser),
    DeleteUser(proto::DeleteUser),
    GetUser(proto::GetUser),
    SetUser(proto::SetUser),
    UserAccess,
    SendBroadcast(proto::SendBroadcast),
}

impl TryFrom<proto::TransactionFrame> for ClientRequest {
    type Error = proto::ProtocolError;

    fn try_from(frame: proto::TransactionFrame) -> Result<Self, Self::Error> {
        match frame.transaction_type()? {
            proto::TransactionType::GetMessages => {
                proto::GetMessages::try_from(frame).map(Into::into)
            }
            proto::TransactionType::PostNewsArticle => {
                proto::PostNews::try_from(frame).map(Into::into)
            }
            proto::TransactionType::GetFileNameList => {
                proto::GetFileNameList::try_from(frame).map(Into::into)
            }
            proto::TransactionType::OldPostNews => proto::PostNews::try_from(frame).map(Into::into),
            proto::TransactionType::SendChat => proto::SendChat::try_from(frame).map(Into::into),
            proto::TransactionType::Login => proto::LoginRequest::try_from(frame).map(Into::into),
            proto::TransactionType::SendInstantMessage => {
                proto::SendInstantMessage::try_from(frame).map(Into::into)
            }
            proto::TransactionType::DisconnectUser => {
                proto::DisconnectUser::try_from(frame).map(Into::into)
            }
            proto::TransactionType::InviteToNewChat => {
                proto::InviteToNewChat::try_from(frame).map(Into::into)
            }
            proto::TransactionType::InviteToChat => {
                proto::InviteToChat::try_from(frame).map(Into::into)
            }
            proto::TransactionType::RejectChatInvite => {
                proto::RejectChatInvite::try_from(frame).map(Into::into)
            }
            proto::TransactionType::JoinChat => proto::JoinChat::try_from(frame).map(Into::into),
            proto::TransactionType::LeaveChat => proto::LeaveChat::try_from(frame).map(Into::into),
            proto::TransactionType::SetChatSubject => {
                proto::SetChatSubject::try_from(frame).map(Into::into)
            }
            proto::TransactionType::Agreed => todo!(),
            proto::TransactionType::DownloadFile => {
                proto::DownloadFile::try_from(frame).map(Into::into)
            }
            proto::TransactionType::UploadFile => {
                proto::UploadFile::try_from(frame).map(Into::into)
            }
            proto::TransactionType::DeleteFile => {
                proto::DeleteFile::try_from(frame).map(Into::into)
            }
            proto::TransactionType::NewFolder => proto::NewFolder::try_from(frame).map(Into::into),
            proto::TransactionType::GetFileInfo => {
                proto::GetFileInfo::try_from(frame).map(Into::into)
            }
            proto::TransactionType::SetFileInfo => {
                proto::SetFileInfo::try_from(frame).map(Into::into)
            }
            proto::TransactionType::MoveFile => proto::MoveFile::try_from(frame).map(Into::into),
            proto::TransactionType::MakeFileAlias => {
                proto::MakeFileAlias::try_from(frame).map(Into::into)
            }
            proto::TransactionType::DownloadFolder => todo!(),
            proto::TransactionType::DownloadBanner => todo!(),
            proto::TransactionType::UploadFolder => todo!(),
            proto::TransactionType::GetUserNameList => {
                proto::GetUserNameList::try_from(frame).map(Into::into)
            }
            proto::TransactionType::GetClientInfoText => {
                proto::GetClientInfoText::try_from(frame).map(Into::into)
            }
            proto::TransactionType::SetClientUserInfo => {
                proto::SetClientUserInfo::try_from(frame).map(Into::into)
            }
            proto::TransactionType::NewUser => proto::NewUser::try_from(frame).map(Into::into),
            proto::TransactionType::DeleteUser => {
                proto::DeleteUser::try_from(frame).map(Into::into)
            }
            proto::TransactionType::GetUser => proto::GetUser::try_from(frame).map(Into::into),
            proto::TransactionType::SetUser => proto::SetUser::try_from(frame).map(Into::into),
            proto::TransactionType::UserBroadcast => {
                proto::SendBroadcast::try_from(frame).map(Into::into)
            }
            // proto::TransactionType::GetNewsCategoryNameList => todo!(),
            // proto::TransactionType::GetNewsArticleNameList => todo!(),
            // proto::TransactionType::DeleteNewsItem => todo!(),
            // proto::TransactionType::NewNewsFolder => todo!(),
            // proto::TransactionType::NewNewsCategory => todo!(),
            // proto::TransactionType::GetNewsArticleData => todo!(),
            // proto::TransactionType::DeleteNewsArticle => todo!(),
            _ => Err(proto::ProtocolError::UnsupportedTransaction(
                frame.header.type_.into(),
            )),
        }
    }
}
