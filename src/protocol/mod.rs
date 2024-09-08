use nom::{
    self,
    IResult,
    multi,
    combinator::{map, verify},
    bytes::{self, streaming::take},
    number::streaming::{be_i16, be_i32, be_i64, be_i8},
};
use deku::prelude::*;
use maplit::hashmap;
use derive_more::{From, Into};
use thiserror::Error;
use std::{
    borrow::Borrow,
    collections::HashMap,
    num::NonZeroU32,
};
use tokio::io::AsyncRead;

mod handshake;
mod transaction;
mod transaction_type;
mod transaction_field;
mod date;
mod parameters;

pub trait HotlineProtocol: Sized {
    fn into_bytes(self) -> Vec<u8>;
    fn from_bytes(bytes: &[u8]) -> BIResult<Self>;
}

trait DekuHotlineProtocol {}

impl <D> HotlineProtocol for D where D: DekuHotlineProtocol, D: DekuContainerWrite, D: for<'a> DekuContainerRead<'a> {
    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().unwrap()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let ((bytes, _bits), value) = <Self as DekuContainerRead>::from_bytes((bytes, 0)).unwrap();
        Ok((bytes, value))
    }
}

type BIResult<'a, T> = IResult<&'a [u8], T>;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("i/o error")]
    IO(#[from] std::io::Error),
    #[error("failed to parse transaction header")]
    ParseHeader,
    #[error("failed to parse transaction body")]
    ParseBody,
    #[error("the transaction body is missing field {0:?}")]
    MissingField(TransactionField),
    #[error("the transaction body has malformed data in field {0:?}")]
    MalformedData(TransactionField),
    #[error("expected transaction {expected:?}, got {encountered:?}")]
    UnexpectedTransaction { expected: i16, encountered: i16 },
    #[error("the transaction header refers to unsupported type {0:?}")]
    UnsupportedTransaction(i16),
    #[error("system error")]
    SystemError,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct ErrorCode(i32);

impl ErrorCode {
    pub fn ok() -> Self {
        Self(0)
    }
}

impl Default for ErrorCode {
    fn default() -> Self {
        Self::ok()
    }
}

pub use handshake::{
    ClientHandshakeRequest,
    ServerHandshakeReply,
    SubProtocolId,
    TransferHandshake,
};
use transaction_field::TransactionField;
pub use transaction_type::TransactionType;
pub use transaction::{
    FieldId,
    Flags,
    IsReply,
    Parameter,
    TransactionBody,
    TransactionFrame,
    TransactionHeader,
    Type,
    DataSize,
    TotalSize,
    Id,
    IntoFrameExt,
};
pub use parameters::{
    ChatId,
    ChatOptions,
    ChatSubject,
    Credential,
    FileComment,
    FileCreatedAt,
    FileModifiedAt,
    FileName,
    FilePath,
    FileType,
    FileSize,
    FileTypeString,
    Creator,
    FileCreatorString,
    IconId,
    Message,
    Nickname,
    Password,
    ReferenceNumber,
    TransactionOptions,
    TransferSize,
    UserFlags,
    UserId,
    UserLogin,
    UserAccess,
    UserNameWithInfo,
    WaitingCount,
};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LoginRequest {
    pub login: Option<UserLogin>,
    pub nickname: Option<Nickname>,
    pub password: Option<Password>,
    pub icon_id: Option<IconId>,
}

impl TryFrom<TransactionFrame> for LoginRequest {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {

        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::Login)?;

        let login = body.borrow_field(TransactionField::UserLogin)
            .map(UserLogin::try_from)
            .transpose()?;

        let nickname = body.borrow_field(TransactionField::UserName)
            .map(Nickname::try_from)
            .transpose()?;

        let password = body.borrow_field(TransactionField::UserPassword)
            .map(Password::try_from)
            .transpose()?;

        let icon_id = body.borrow_field(TransactionField::UserIconId)
            .map(IconId::try_from)
            .transpose()?;

        Ok(Self { login, nickname, password, icon_id })
    }
}

impl From<LoginRequest> for TransactionBody {
    fn from(val: LoginRequest) -> Self {
        let LoginRequest { login, nickname, password, icon_id } = val;

        let login = login.map(UserLogin::into);
        let password = password.map(Password::into);
        let nickname = nickname.map(Nickname::into);
        let icon_id = icon_id.map(IconId::into);

        let parameters = [login, nickname, password, icon_id].into_iter()
            .flat_map(Option::into_iter)
            .collect::<Vec<Parameter>>();

        parameters.into()
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LoginReply(ProtocolVersion);

impl LoginReply {
    pub fn new(version: i16) -> Self {
        Self(ProtocolVersion(version))
    }
}

impl Default for LoginReply {
    fn default() -> Self {
        Self::new(123)
    }
}

impl From<LoginReply> for TransactionFrame {
    fn from(val: LoginReply) -> Self {
        let LoginReply(version) = val;
        let header = TransactionType::Login.into();
        let body = vec![version.into()].into();
        Self { header, body }
    }
}

#[derive(Debug, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProtocolVersion(i16);

impl From<ProtocolVersion> for Parameter {
    fn from(val: ProtocolVersion) -> Self {
        Parameter::new_int(
            TransactionField::Version,
            val.0,
        )
    }
}

#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ShowAgreement {
    pub agreement: Option<ServerAgreement>,
    pub banner: Option<ServerBanner>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ServerAgreement(pub Vec<u8>);

impl From<ServerAgreement> for Parameter {
    fn from(val: ServerAgreement) -> Self {
        Parameter::new(
            TransactionField::Data,
            val.0,
        )
    }
}

impl TryFrom<&Parameter> for ServerAgreement {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let data = take_if_matches(
            parameter.clone(),
            TransactionField::ServerAgreement,
        )?;
        Ok(Self(data))
    }
}

enum ServerBannerType {
    Url,
    Data,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ServerBanner {
    URL(Vec<u8>),
    Data(Vec<u8>),
}

impl TryFrom<TransactionBody> for ShowAgreement {
    type Error = ProtocolError;
    fn try_from(body: TransactionBody) -> Result<Self, Self::Error> {

        let agreement = body.borrow_field(TransactionField::ServerAgreement)
            .map(ServerAgreement::try_from)
            .transpose()?;

        let no_agreement = body.borrow_field(TransactionField::NoServerAgreement)
            .is_some();

        let agreement = if no_agreement {
            None
        } else {
            agreement
        };

        let banner = None;

        Ok(Self { agreement, banner })
    }
}

impl TryFrom<&Parameter> for ServerBannerType {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let field_data: &[u8] = parameter.borrow();
        match *field_data {
            [1] => {
                Ok(ServerBannerType::Url)
            },
            [0] => {
                Ok(ServerBannerType::Data)
            },
            _ => {
                Err(ProtocolError::MalformedData(TransactionField::ServerBannerType))
            }
        }
    }
}

impl From<TransactionType> for Type {
    fn from(_type: TransactionType) -> Self {
        Self::from(_type as i16)
    }
}

impl From<TransactionField> for FieldId {
    fn from(field: TransactionField) -> Self {
        Self::from(field as i16)
    }
}

impl From<ShowAgreement> for TransactionBody {
    fn from(val: ShowAgreement) -> Self {
        let parameter = if let Some(agreement) = val.agreement {
            agreement.into()
        } else {
            Parameter::new_int(
                TransactionField::NoServerAgreement,
                1i16,
            )
        };
        vec![parameter].into()
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SetClientUserInfo {
    pub username: Nickname,
    pub icon_id: IconId,
}

impl TryFrom<TransactionFrame> for SetClientUserInfo {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {

        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::SetClientUserInfo)?;

        let username = body.require_field(TransactionField::UserName)
            .and_then(Nickname::try_from)?;

        let icon_id = body.require_field(TransactionField::UserIconId)
            .and_then(IconId::try_from)?;

        Ok(Self { username, icon_id })
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NotifyUserChange {
    pub user_id: UserId,
    pub user_flags: UserFlags,
    pub username: Nickname,
    pub icon_id: IconId,
}

impl From<NotifyUserChange> for TransactionFrame {
    fn from(val: NotifyUserChange) -> Self {
        let header = TransactionHeader {
            type_: TransactionType::NotifyUserChange.into(),
            ..Default::default()
        };
        let NotifyUserChange {
            user_id,
            username,
            icon_id,
            user_flags,
        } = val;
        let body = vec![
            user_id.into(),
            icon_id.into(),
            user_flags.into(),
            username.into(),
        ].into();
        Self { header, body }
    }
}

impl From<&UserNameWithInfo> for NotifyUserChange {
    fn from(user: &UserNameWithInfo) -> Self {
        let UserNameWithInfo {
            icon_id,
            user_flags,
            user_id,
            username,
        } = user.clone();
        Self {
            icon_id,
            user_flags,
            user_id,
            username,
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NotifyUserDelete {
    pub user_id: UserId,
}

impl From<NotifyUserDelete> for TransactionFrame {
    fn from(val: NotifyUserDelete) -> Self {
        let header = TransactionHeader {
            type_: TransactionType::NotifyUserDelete.into(),
            ..Default::default()
        };
        let NotifyUserDelete { user_id } = val;
        let body = vec![user_id.into()].into();
        Self { header, body }
    }
}

impl From<UserId> for NotifyUserDelete {
    fn from(user_id: UserId) -> Self {
        Self { user_id }
    }
}

impl From<&UserNameWithInfo> for NotifyUserDelete {
    fn from(user: &UserNameWithInfo) -> Self {
        user.user_id.into()
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NotifyChatUserChange {
    pub chat_id: ChatId,
    pub user_id: UserId,
    pub icon_id: IconId,
    pub user_flags: UserFlags,
    pub user_name: Nickname,
}

impl From<NotifyChatUserChange> for TransactionFrame {
    fn from(val: NotifyChatUserChange) -> Self {
        let header = TransactionHeader {
            type_: TransactionType::NotifyChatUserChange.into(),
            ..Default::default()
        };
        let NotifyChatUserChange {
            chat_id,
            user_id,
            icon_id,
            user_flags,
            user_name,
        } = val;
        let body = vec![
            chat_id.into(),
            user_id.into(),
            icon_id.into(),
            user_flags.into(),
            user_name.into(),
        ].into();
        Self { header, body }
    }
}

impl From<(ChatId, &UserNameWithInfo)> for NotifyChatUserChange {
    fn from((chat_id, user): (ChatId, &UserNameWithInfo)) -> Self {
        let UserNameWithInfo {
            user_id,
            icon_id,
            user_flags,
            username,
        } = user.clone();
        Self {
            chat_id,
            user_id,
            icon_id,
            user_flags,
            user_name: username,
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NotifyChatUserDelete {
    pub chat_id: ChatId,
    pub user_id: UserId,
    pub icon_id: IconId,
    pub user_flags: UserFlags,
    pub user_name: Nickname,
}

impl From<NotifyChatUserDelete> for TransactionFrame {
    fn from(val: NotifyChatUserDelete) -> Self {
        let header = TransactionHeader {
            type_: TransactionType::NotifyChatUserDelete.into(),
            ..Default::default()
        };
        let NotifyChatUserDelete {
            chat_id,
            user_id,
            icon_id,
            user_flags,
            user_name,
        } = val;
        let body = vec![
            chat_id.into(),
            user_id.into(),
            icon_id.into(),
            user_flags.into(),
            user_name.into(),
        ].into();
        Self { header, body }
    }
}

impl From<(ChatId, &UserNameWithInfo)> for NotifyChatUserDelete {
    fn from((chat_id, user): (ChatId, &UserNameWithInfo)) -> Self {
        let UserNameWithInfo {
            user_id,
            icon_id,
            user_flags,
            username,
        } = user.clone();
        Self {
            chat_id,
            user_id,
            icon_id,
            user_flags,
            user_name: username,
        }
    }
}

#[derive(Debug, From, Into, PartialEq, Eq, PartialOrd, Ord)]
pub struct NotifyChatSubject {
    pub chat_id: ChatId,
    pub subject: ChatSubject,
}

impl From<NotifyChatSubject> for TransactionFrame {
    fn from(val: NotifyChatSubject) -> Self {
        let header = TransactionHeader {
            type_: TransactionType::NotifyChatSubject.into(),
            ..Default::default()
        };
        let NotifyChatSubject { chat_id, subject } = val;
        let body = vec![
            chat_id.into(),
            subject.into(),
        ].into();
        Self { header, body }
    }
}

impl TryFrom<TransactionFrame> for NotifyChatSubject {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame { body, .. } = frame;
        let chat_id = body.require_field(TransactionField::ChatId)
            .and_then(ChatId::try_from)?;
        let subject = body.require_field(TransactionField::ChatSubject)
            .and_then(ChatSubject::try_from)?;
        Ok(Self { chat_id, subject })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct GetUserNameList;

impl TryFrom<TransactionFrame> for GetUserNameList {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        frame.require_transaction_type(TransactionType::GetUserNameList)?;
        Ok(Self)
    }
}

impl From<GetUserNameList> for TransactionBody {
    fn from(_: GetUserNameList) -> Self {
        Default::default()
    }
}

#[derive(Debug, Default)]
pub struct GetUserNameListReply(Vec<UserNameWithInfo>);

impl GetUserNameListReply {
    pub fn empty() -> Self {
        Self(vec![])
    }
    pub fn single(user: UserNameWithInfo) -> Self {
        Self::with_users(vec![user])
    }
    pub fn with_users(users: Vec<UserNameWithInfo>) -> Self {
        Self(users)
    }
}

impl From<GetUserNameListReply> for TransactionFrame {
    fn from(val: GetUserNameListReply) -> Self {
        let header = TransactionHeader {
            type_: TransactionType::GetUserNameList.into(),
            is_reply: IsReply::reply(),
            ..Default::default()
        };
        let GetUserNameListReply(users) = val;
        let body = users.into_iter()
            .map(UserNameWithInfo::into)
            .collect();
        Self { header, body }
    }
}

#[derive(Debug, Clone)]
pub struct DisconnectUser {
    pub user_id: UserId,
    pub options: Option<TransactionOptions>,
    pub data: Option<Vec<u8>>,
}

impl TryFrom<TransactionFrame> for DisconnectUser {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body,
            ..
        } = frame.require_transaction_type(TransactionType::DisconnectUser)?;

        let user_id = body.require_field(TransactionField::UserId)
            .and_then(UserId::try_from)?;
        let options = body.borrow_field(TransactionField::Options)
            .and_then(Parameter::int)
            .and_then(|i| i.i32())
            .map(TransactionOptions::from);
        let data = body.borrow_field(TransactionField::Data)
            .cloned()
            .map(Parameter::take);

        Ok(Self { user_id, options, data })
    }
}

impl From<DisconnectUser> for TransactionFrame {
    fn from(val: DisconnectUser) -> Self {
        let header = TransactionType::DisconnectUser.into();
        let DisconnectUser { user_id, options, data } = val;
        let body = [
            Some(Parameter::from(user_id)),
            options.map(Parameter::from),
            data.map(Parameter::new_data),
        ].into_iter()
            .flat_map(Option::into_iter)
            .collect::<TransactionBody>();
        Self { header, body }
    }
}

#[derive(Debug)]
pub struct GetMessages;

impl TryFrom<TransactionFrame> for GetMessages {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        frame.require_transaction_type(TransactionType::GetMessages)?;
        Ok(Self)
    }
}

impl From<GetMessages> for TransactionFrame {
    fn from(_: GetMessages) -> Self {
        Self::empty(TransactionType::GetMessages)
    }
}

#[derive(Debug)]
pub struct GetMessagesReply(Vec<Message>);

impl GetMessagesReply {
    pub fn empty() -> Self {
        Self(vec![])
    }
    pub fn single(message: Message) -> Self {
        Self::with_messages(vec![message])
    }
    pub fn with_messages(messages: Vec<Message>) -> Self {
        Self(messages)
    }
}

impl From<GetMessagesReply> for TransactionFrame {
    fn from(val: GetMessagesReply) -> Self {
        let header = TransactionHeader {
            type_: TransactionType::GetMessages.into(),
            is_reply: IsReply::reply(),
            ..Default::default()
        };
        let GetMessagesReply(messages) = val;
        let body = messages.into_iter()
            .map(Message::into)
            .collect();
        Self { header, body }
    }
}

#[derive(Debug, From, Into)]
pub struct PostNews(pub Message);

impl TryFrom<TransactionFrame> for PostNews {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame { body, .. } = frame.require_transaction_type(
            TransactionType::OldPostNews
        )?;
        let post = body.require_field(TransactionField::Data)
            .map(Message::from)?;
        Ok(Self(post))
    }
}

impl From<PostNews> for TransactionFrame {
    fn from(val: PostNews) -> Self {
        let header = TransactionType::Reply.into();
        let PostNews(post) = val;
        let body = vec![post.into()].into();
        Self { header, body }
    }
}

#[derive(Debug, Clone, From, Into)]
pub struct NotifyNewsMessage(Message);

impl TryFrom<TransactionFrame> for NotifyNewsMessage {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame { body, .. } = frame.require_transaction_type(
            TransactionType::NewMessage
        )?;
        let post = body.require_field(TransactionField::Data)
            .map(Message::from)?;
        Ok(Self(post))
    }
}

impl From<NotifyNewsMessage> for TransactionFrame {
    fn from(val: NotifyNewsMessage) -> Self {
        let header = TransactionType::NewMessage.into();
        let NotifyNewsMessage(post) = val;
        let body = vec![post.into()].into();
        Self { header, body }
    }
}

#[derive(Debug, From, Into)]
pub struct GetFileNameList(pub FilePath);

impl TryFrom<TransactionFrame> for GetFileNameList {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {

        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::GetFileNameList)?;

        let path = body.borrow_field(TransactionField::FilePath)
            .try_into()?;

        Ok(Self(path))
    }
}

impl From<GetFileNameList> for TransactionFrame {
    fn from(_: GetFileNameList) -> Self {
        Self::empty(TransactionType::GetFileNameList)
    }
}

#[derive(Debug)]
pub struct GetFileNameListReply(Vec<FileNameWithInfo>);

impl GetFileNameListReply {
    pub fn empty() -> Self {
        Self(vec![])
    }
    pub fn single(file: FileNameWithInfo) -> Self {
        Self::with_files(vec![file])
    }
    pub fn with_files(files: Vec<FileNameWithInfo>) -> Self {
        Self(files)
    }
}

impl From<GetFileNameListReply> for TransactionFrame {
    fn from(val: GetFileNameListReply) -> Self {
        let header = TransactionHeader {
            type_: TransactionType::GetFileNameList.into(),
            is_reply: IsReply::reply(),
            ..Default::default()
        };
        let body = val.0.into_iter()
            .map(FileNameWithInfo::into)
            .collect();
        Self { header, body }
    }
}

#[derive(Debug)]
pub struct FileNameWithInfo {
    pub file_type: FileType,
    pub creator: Creator,
    pub file_size: FileSize,
    pub name_script: NameScript,
    pub file_name: Vec<u8>,
}

impl From<FileNameWithInfo> for Parameter {
    fn from(val: FileNameWithInfo) -> Self {
        let filename_size = val.file_name.len() as i16;
        let data = [
            &val.file_type.0[..],
            &val.creator.0[..],
            &(i32::from(val.file_size)).to_be_bytes()[..],
            &[0u8; 4][..],
            &val.name_script.to_bytes().unwrap(),
            &filename_size.to_be_bytes()[..],
            &val.file_name[..],
        ].iter()
            .flat_map(|bytes| bytes.iter())
            .copied()
            .collect();
        Parameter::new(
            TransactionField::FileNameWithInfo,
            data,
        )
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct NameScript(i16);

#[derive(Debug, Clone)]
pub struct GetFileInfo {
    pub filename: FileName,
    pub path: FilePath,
}

impl TryFrom<TransactionFrame> for GetFileInfo {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::GetFileInfo)?;

        let filename = body.require_field(TransactionField::FileName)
            .map(FileName::from)?;

        let path = body.borrow_field(TransactionField::FilePath)
            .try_into()?;

        Ok(Self { filename, path })
    }
}

impl From<GetFileInfo> for TransactionFrame {
    fn from(_: GetFileInfo) -> Self {
        Self::empty(TransactionType::GetFileNameList)
    }
}

#[derive(Debug)]
pub struct GetFileInfoReply {
    pub filename: FileName,
    pub size: FileSize,
    pub type_code: FileType,
    pub creator: FileCreatorString,
    pub comment: FileComment,
    pub created_at: FileCreatedAt,
    pub modified_at: FileModifiedAt,
}

impl From<GetFileInfoReply> for TransactionFrame {
    fn from(val: GetFileInfoReply) -> Self {
        let header = TransactionType::GetFileInfo.into();
        let type_string = FileTypeString::from(&val.type_code);
        let body = vec![
            val.filename.into(),
            val.type_code.into(),
            val.creator.into(),
            val.created_at.into(),
            val.modified_at.into(),
            val.comment.into(),
            val.size.into(),
            type_string.into(),
        ].into();
        Self { header, body }
    }
}

#[derive(Debug, Clone)]
pub struct SetFileInfo {
    pub filename: FileName,
    pub path: FilePath,
    pub new_name: Option<FileName>,
    pub new_comment: Option<FileComment>,
}

impl TryFrom<TransactionFrame> for SetFileInfo {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::SetFileInfo)?;

        let filename = body.require_field(TransactionField::FileName)
            .map(FileName::from)?;
        let path = body.borrow_field(TransactionField::FilePath)
            .try_into()?;
        let new_name = body.borrow_field(TransactionField::FileNewName)
            .map(FileName::from);
        let new_comment = body.borrow_field(TransactionField::FileComment)
            .and_then(|param| FileComment::try_from(param).ok());

        Ok(Self { filename, path, new_name, new_comment })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SetFileInfoReply;

impl TryFrom<TransactionFrame> for SetFileInfoReply {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        frame.require_transaction_type(TransactionType::DeleteFile)?;
        Ok(Self)
    }
}

impl From<SetFileInfoReply> for TransactionFrame {
    fn from(_: SetFileInfoReply) -> Self {
        Self::empty(TransactionType::SetFileInfo)
    }
}

#[derive(Debug, Clone)]
pub struct SendChat {
    pub options: ChatOptions,
    pub chat_id: Option<ChatId>,
    pub message: Vec<u8>,
}

impl TryFrom<TransactionFrame> for SendChat {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {

        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::SendChat)?;

        let options = body.borrow_field(TransactionField::ChatOptions)
            .map(ChatOptions::try_from)
            .transpose()?
            .unwrap_or_default();

        let chat_id = body.borrow_field(TransactionField::ChatId)
            .map(ChatId::try_from)
            .transpose()?;

        let message = body.require_field(TransactionField::Data)
            .map(|p| p.clone().take())?;

        let chat = Self {
            options,
            chat_id,
            message,
        };

        Ok(chat)
    }
}

impl From<SendChat> for TransactionFrame {
    fn from(val: SendChat) -> Self {
        let header = TransactionType::SendChat.into();
        let SendChat { message, chat_id, options } = val;
        let body = vec![
            Some(Parameter::new(TransactionField::Data, message)),
            chat_id.map(ChatId::into),
            Some(options.into()),
        ].into_iter()
            .flat_map(Option::into_iter)
            .collect();
        Self { header, body }
    }
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub chat_id: Option<ChatId>,
    pub message: Vec<u8>,
}

impl TryFrom<TransactionFrame> for ChatMessage {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {

        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::ChatMessage)?;

        let chat_id = body.borrow_field(TransactionField::ChatId)
            .map(ChatId::try_from)
            .transpose()?;

        let message = body.require_field(TransactionField::Data)
            .map(|p| p.clone().take())?;

        Ok(Self { chat_id, message })
    }
}

impl From<ChatMessage> for TransactionFrame {
    fn from(val: ChatMessage) -> Self {
        let header = TransactionType::ChatMessage.into();
        let ChatMessage { message, chat_id } = val;
        let message = Parameter::new(
            TransactionField::Data,
            message,
        );
        let chat_id = chat_id.map(ChatId::into);
        let body = [
            Some(message),
            chat_id,
        ].into_iter()
            .flat_map(Option::into_iter)
            .collect();
        Self { header, body }
    }
}

#[derive(Debug)]
pub struct ServerMessage {
    pub user_id: Option<UserId>,
    pub user_name: Option<Nickname>,
    pub message: Vec<u8>,
}

impl From<ServerMessage> for TransactionFrame {
    fn from(val: ServerMessage) -> Self {
        let header = TransactionType::ServerMessage.into();
        let ServerMessage { message, user_id, user_name } = val;
        let message = Parameter::new(
            TransactionField::Data,
            message,
        );
        let user_id = user_id.map(UserId::into);
        let user_name = user_name.map(Nickname::into);
        let body = vec![
            Some(message),
            user_id,
            user_name,
        ].into_iter()
            .flat_map(Option::into_iter)
            .collect();
        Self { header, body }
    }
}

#[derive(Debug)]
pub struct DisconnectMessage {
    pub message: Vec<u8>,
}

impl TryFrom<TransactionFrame> for DisconnectMessage {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::SendInstantMessage)?;
        let message = body.require_field(TransactionField::Data)
            .map(|p| p.clone().take())?;
        Ok(Self { message })
    }
}

impl From<DisconnectMessage> for TransactionFrame {
    fn from(val: DisconnectMessage) -> Self {
        let header = TransactionType::DisconnectMessage.into();
        let DisconnectMessage { message } = val;
        let message = Parameter::new(
            TransactionField::Data,
            message,
        );
        let body = vec![message].into();
        Self { header, body }
    }
}

#[derive(Debug)]
pub struct SendInstantMessage {
    pub user_id: UserId,
    pub message: Vec<u8>,
}

impl TryFrom<TransactionFrame> for SendInstantMessage {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {

        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::SendInstantMessage)?;

        let user_id = body.require_field(TransactionField::UserId)
            .and_then(UserId::try_from)?;

        let message = body.require_field(TransactionField::Data)
            .map(|p| p.clone().take())?;

        Ok(Self { user_id, message })
    }
}

impl From<SendInstantMessage> for TransactionFrame {
    fn from(val: SendInstantMessage) -> Self {
        let header = TransactionType::SendChat.into();
        let SendInstantMessage { user_id, message } = val;
        let body = vec![
            user_id.into(),
            Parameter::new(TransactionField::Data, message),
        ].into();
        Self { header, body }
    }
}

pub struct SendInstantMessageReply;

impl TryFrom<TransactionFrame> for SendInstantMessageReply {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        frame.require_transaction_type(TransactionType::Reply)?;
        Ok(Self)
    }
}

impl From<SendInstantMessageReply> for TransactionFrame {
    fn from(_: SendInstantMessageReply) -> Self {
        Self::empty(TransactionHeader::default())
    }
}

#[derive(Debug, From, Into)]
pub struct InviteToNewChat(Vec<UserId>);

impl TryFrom<TransactionFrame> for InviteToNewChat {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let frame = frame.require_transaction_type(TransactionType::InviteToNewChat)?;
        let TransactionFrame { body, .. } = frame;

        let user_ids: Result<_, _> = body.borrow_fields(TransactionField::UserId)
            .into_iter()
            .map(UserId::try_from)
            .collect();

        Ok(Self(user_ids?))
    }
}

impl From<InviteToNewChat> for TransactionFrame {
    fn from(val: InviteToNewChat) -> Self {
        let header = TransactionType::InviteToNewChat.into();
        let InviteToNewChat(user_ids) = val;
        let body = user_ids.into_iter()
            .map(UserId::into)
            .collect();
        Self { header, body }
    }
}

#[derive(Debug)]
pub struct InviteToNewChatReply {
    pub chat_id: ChatId,
    pub user_id: UserId,
    pub icon_id: IconId,
    pub flags: UserFlags,
    pub user_name: Nickname,
}

impl TryFrom<TransactionFrame> for InviteToNewChatReply {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let frame = frame.require_transaction_type(TransactionType::Reply)?;
        let TransactionFrame { body, .. } = frame;

        let chat_id = body.require_field(TransactionField::ChatId)
            .and_then(ChatId::try_from)?;

        let user_id = body.require_field(TransactionField::UserId)
            .and_then(UserId::try_from)?;

        let icon_id = body.require_field(TransactionField::UserIconId)
            .and_then(IconId::try_from)?;

        let flags = body.require_field(TransactionField::UserFlags)
            .and_then(UserFlags::try_from)?;

        let user_name = body.require_field(TransactionField::UserName)
            .and_then(Nickname::try_from)?;

        Ok(
            Self {
                chat_id,
                user_id,
                icon_id,
                flags,
                user_name,
            }
        )
    }
}

impl From<InviteToNewChatReply> for TransactionFrame {
    fn from(val: InviteToNewChatReply) -> Self {
        let header = TransactionType::InviteToNewChat.into();
        let InviteToNewChatReply {
            chat_id,
            user_id,
            user_name,
            icon_id,
            flags,
        } = val;
        let body = vec![
            chat_id.into(),
            user_id.into(),
            icon_id.into(),
            user_name.into(),
            flags.into(),
        ].into();
        Self { header, body }
    }
}

#[derive(Debug)]
pub struct InviteToChat {
    pub user_id: UserId,
    pub chat_id: ChatId,
}

impl TryFrom<TransactionFrame> for InviteToChat {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let frame = frame.require_transaction_type(TransactionType::InviteToChat)?;
        let TransactionFrame { body, .. } = frame;

        let user_id = body.require_field(TransactionField::UserId)
            .and_then(UserId::try_from)?;

        let chat_id = body.require_field(TransactionField::ChatId)
            .and_then(ChatId::try_from)?;

        Ok(Self { user_id, chat_id })
    }
}

impl From<InviteToChat> for TransactionFrame {
    fn from(val: InviteToChat) -> Self {
        let header = TransactionType::InviteToChat.into();
        let InviteToChat { user_id, chat_id } = val;
        let body = vec![
            user_id.into(),
            chat_id.into(),
        ].into();
        Self { header, body }
    }
}

#[derive(Debug, From, Into)]
pub struct JoinChat(ChatId);

impl TryFrom<TransactionFrame> for JoinChat {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let frame = frame.require_transaction_type(TransactionType::JoinChat)?;
        let TransactionFrame { body, .. } = frame;

        let chat_id = body.require_field(TransactionField::ChatId)
            .and_then(ChatId::try_from)?;

        Ok(Self(chat_id))
    }
}

impl From<JoinChat> for TransactionFrame {
    fn from(val: JoinChat) -> Self {
        let header = TransactionType::JoinChat.into();
        let JoinChat(chat_id) = val;
        let body = vec![chat_id.into()].into();
        Self { header, body }
    }
}

#[derive(Debug, From, Into)]
pub struct JoinChatReply {
    subject: Option<ChatSubject>,
    users: Vec<UserNameWithInfo>,
}

impl TryFrom<TransactionFrame> for JoinChatReply {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let frame = frame.require_transaction_type(TransactionType::JoinChat)?;
        let TransactionFrame { body, .. } = frame;

        let subject = body.borrow_field(TransactionField::ChatSubject)
            .map(ChatSubject::try_from)
            .transpose()?;

        let users = body.borrow_fields(TransactionField::UserNameWithInfo)
            .into_iter()
            .map(UserNameWithInfo::try_from)
            .collect::<Result<_, _>>()?;

        Ok(Self { subject, users })
    }
}

impl From<JoinChatReply> for TransactionFrame {
    fn from(val: JoinChatReply) -> Self {
        let header = TransactionType::Reply.into();
        let JoinChatReply { subject, users } = val;
        let subject = subject.map(ChatSubject::into);
        let users: Vec<Parameter> = users.into_iter()
            .map(UserNameWithInfo::into)
            .collect();
        let body = subject.into_iter()
            .chain(users)
            .collect();
        Self { header, body }
    }
}

#[derive(Debug, From, Into)]
pub struct LeaveChat(ChatId);

impl TryFrom<TransactionFrame> for LeaveChat {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let frame = frame.require_transaction_type(TransactionType::LeaveChat)?;
        let TransactionFrame { body, .. } = frame;

        let chat_id = body.require_field(TransactionField::ChatId)
            .and_then(ChatId::try_from)?;

        Ok(Self(chat_id))
    }
}

impl From<LeaveChat> for TransactionFrame {
    fn from(val: LeaveChat) -> Self {
        let header = TransactionType::LeaveChat.into();
        let LeaveChat(chat_id) = val;
        let body = vec![chat_id.into()].into();
        Self { header, body }
    }
}

#[derive(Debug, From, Into)]
pub struct SetChatSubject(ChatId, ChatSubject);

impl TryFrom<TransactionFrame> for SetChatSubject {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let frame = frame.require_transaction_type(TransactionType::SetChatSubject)?;
        let TransactionFrame { body, .. } = frame;

        let chat_id = body.require_field(TransactionField::ChatId)
            .and_then(ChatId::try_from)?;

        let subject = body.require_field(TransactionField::ChatSubject)
            .and_then(ChatSubject::try_from)?;

        Ok(Self(chat_id, subject))
    }
}

impl From<SetChatSubject> for TransactionFrame {
    fn from(val: SetChatSubject) -> Self {
        let header = TransactionType::SetChatSubject.into();
        let SetChatSubject(chat_id, subject) = val;
        let body = vec![
            chat_id.into(),
            subject.into(),
        ].into();
        Self { header, body }
    }
}

#[derive(Debug, Clone)]
pub struct GetClientInfoText {
    pub user_id: UserId,
}

impl TryFrom<TransactionFrame> for GetClientInfoText {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {

        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::GetClientInfoText)?;

        let user_id = body.require_field(TransactionField::UserId)
            .and_then(UserId::try_from)?;

        Ok(Self { user_id })
    }
}

impl From<GetClientInfoText> for TransactionFrame {
    fn from(val: GetClientInfoText) -> Self {
        let header = TransactionType::GetClientInfoText.into();
        let GetClientInfoText { user_id, .. } = val;
        let body = vec![user_id.into()].into();
        Self { header, body }
    }
}

#[derive(Debug)]
pub struct GetClientInfoTextReply {
    pub user_name: Nickname,
    pub text: Vec<u8>,
}

impl TryFrom<TransactionFrame> for GetClientInfoTextReply {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {

        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::GetClientInfoText)?;

        let user_name = body.require_field(TransactionField::UserName)
            .and_then(Nickname::try_from)?;

        let text = body.require_field(TransactionField::Data)
            .map(|p| p.clone().take())?;

        Ok(Self { user_name, text })
    }
}

impl From<GetClientInfoTextReply> for TransactionFrame {
    fn from(val: GetClientInfoTextReply) -> Self {
        let header = TransactionType::GetClientInfoText.into();
        let GetClientInfoTextReply { user_name, text } = val;
        let body = vec![
            user_name.into(),
            Parameter::new(TransactionField::Data, text),
        ].into();
        Self { header, body }
    }
}

#[derive(Debug)]
pub struct SendBroadcast {
    pub message: Vec<u8>,
}

impl TryFrom<TransactionFrame> for SendBroadcast {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::UserBroadcast)?;

        let message = body.require_field(TransactionField::Data)
            .map(|p| p.clone().take())?;

        Ok(Self { message })
    }
}

impl From<SendBroadcast> for TransactionFrame {
    fn from(val: SendBroadcast) -> Self {
        let header = TransactionType::GetClientInfoText.into();
        let SendBroadcast { message } = val;
        let body = vec![
            Parameter::new(TransactionField::Data, message),
        ].into();
        Self { header, body }
    }
}

pub struct GenericReply;

impl TryFrom<TransactionFrame> for GenericReply {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        frame.require_transaction_type(TransactionType::Reply)?;
        Ok(Self)
    }
}

impl From<GenericReply> for TransactionFrame {
    fn from(_: GenericReply) -> Self {
        Self::empty(TransactionHeader::default())
    }
}

#[derive(Debug)]
pub struct NewUser {
    pub login: UserLogin,
    pub password: Password,
    pub name: Nickname,
    pub access: UserAccess,
}

impl From<NewUser> for TransactionFrame {
    fn from(val: NewUser) -> Self {
        let header = TransactionType::NewUser.into();
        let NewUser { login, password, name, access } = val;
        let body = vec![
            login.into(),
            password.into(),
            name.into(),
            access.into(),
        ].into();
        Self { header, body }
    }
}

impl TryFrom<TransactionFrame> for NewUser {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::NewUser)?;

        let login = body.require_field(TransactionField::UserLogin)
            .and_then(UserLogin::try_from)?;
        let password = body.require_field(TransactionField::UserPassword)
            .and_then(Password::try_from)?;
        let name = body.require_field(TransactionField::UserName)
            .and_then(Nickname::try_from)?;
        let access = body.require_field(TransactionField::UserAccess)
            .and_then(UserAccess::try_from)?;

        Ok(Self { login, password, name, access })
    }
}

#[derive(Debug)]
pub struct DeleteUser(pub UserLogin);

impl TryFrom<TransactionFrame> for DeleteUser {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::DeleteUser)?;

        let login = body.require_field(TransactionField::UserLogin)
            .cloned()
            .map(Parameter::take)
            .map(UserLogin::new)?;

        Ok(Self(login))
    }
}

impl From<DeleteUser> for TransactionFrame {
    fn from(val: DeleteUser) -> Self {
        let header = TransactionType::DeleteUser.into();
        let DeleteUser(user) = val;
        let body = vec![user.into()].into();
        Self { header, body }
    }
}

#[derive(Debug)]
pub struct SetUser {
    pub login: UserLogin,
    pub password: Password,
    pub name: Nickname,
    pub access: UserAccess,
}

impl TryFrom<TransactionFrame> for SetUser {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::SetUser)?;

        let login = body.require_field(TransactionField::UserLogin)
            .and_then(UserLogin::try_from)?;
        let password = body.require_field(TransactionField::UserPassword)
            .and_then(Password::try_from)?;
        let name = body.require_field(TransactionField::UserName)
            .and_then(Nickname::try_from)?;
        let access = body.require_field(TransactionField::UserAccess)
            .and_then(UserAccess::try_from)?;

        Ok(Self { login, password, name, access })
    }
}

impl From<SetUser> for TransactionFrame {
    fn from(val: SetUser) -> Self {
        let header = TransactionType::SetUser.into();
        let SetUser { login, password, name, access } = val;
        let body = vec![
            login.into(),
            password.into(),
            name.into(),
            access.into(),
        ].into();
        Self { header, body }
    }
}

#[derive(Debug)]
pub struct GetUser(pub UserLogin);

impl TryFrom<TransactionFrame> for GetUser {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::GetUser)?;

        let login = body.require_field(TransactionField::UserLogin)
            .cloned()
            .map(Parameter::take)
            .map(UserLogin::new)?;

        Ok(Self(login))
    }
}

impl From<GetUser> for TransactionFrame {
    fn from(val: GetUser) -> Self {
        let header = TransactionType::GetUser.into();
        let GetUser(user) = val;
        let body = vec![user.into()].into();
        Self { header, body }
    }
}

#[derive(Debug)]
pub struct GetUserReply {
    pub username: Nickname,
    pub user_login: UserLogin,
    pub user_password: Password,
    pub user_access: UserAccess,
}

impl TryFrom<TransactionFrame> for GetUserReply {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::GetUser)?;

        let username = body.require_field(TransactionField::UserName)
            .and_then(Nickname::try_from)?;
        let user_login = body.require_field(TransactionField::UserLogin)
            .and_then(UserLogin::try_from)?;
        let user_password = body.require_field(TransactionField::UserPassword)
            .and_then(Password::try_from)?;
        let user_access = body.require_field(TransactionField::UserAccess)
            .and_then(UserAccess::try_from)?;

        Ok(Self { username, user_login, user_password, user_access })
    }
}

impl From<GetUserReply> for TransactionFrame {
    fn from(val: GetUserReply) -> Self {
        let GetUserReply {
            username,
            user_login,
            user_password,
            user_access,
        } = val;
        let body: TransactionBody = vec![
            username.into(),
            user_login.into(),
            user_password.into(),
            user_access.into(),
        ].into();
        Self::new(TransactionType::GetUser, body)
    }
}

#[derive(Debug)]
pub struct DownloadFile {
    pub filename: FileName,
    pub file_path: FilePath,
    // TODO: resume
    // TODO: options
}

impl TryFrom<TransactionFrame> for DownloadFile {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::DownloadFile)?;

        let filename = body.require_field(TransactionField::FileName)
            .map(FileName::from)?;
        let file_path = body.borrow_field(TransactionField::FilePath)
            .try_into()?;

        Ok(Self { filename, file_path })
    }
}

impl From<DownloadFile> for TransactionFrame {
    fn from(val: DownloadFile) -> Self {
        let DownloadFile { filename, file_path } = val;
        let body = [
            Some(filename.into()),
            file_path.into(),
        ]
            .into_iter()
            .flat_map(Option::into_iter)
            .collect::<TransactionBody>();
        Self::new(TransactionType::DownloadFile, body)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct DownloadFileReply {
    pub transfer_size: TransferSize,
    pub file_size: FileSize,
    pub reference: ReferenceNumber,
    pub waiting_count: Option<WaitingCount>,
}

impl TryFrom<TransactionFrame> for DownloadFileReply {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame { body, ..  } = frame;

        let transfer_size = body.require_field(TransactionField::TransferSize)
            .and_then(TransferSize::try_from)?;
        let file_size = body.require_field(TransactionField::FileSize)
            .and_then(FileSize::try_from)
            .unwrap_or_else(|_| FileSize::default());
        let reference = body.require_field(TransactionField::ReferenceNumber)
            .and_then(ReferenceNumber::try_from)?;
        let waiting_count = body.borrow_field(TransactionField::WaitingCount)
            .map(WaitingCount::try_from)
            .transpose()?;

        Ok(Self { transfer_size, file_size, reference, waiting_count })
    }
}

impl From<DownloadFileReply> for TransactionFrame {
    fn from(val: DownloadFileReply) -> Self {
        let DownloadFileReply { transfer_size, file_size, reference, waiting_count } = val;
        let body = [
            Some(transfer_size.into()),
            Some(file_size.into()),
            Some(reference.into()),
            Some(waiting_count.unwrap_or_default().into()),
        ]
            .into_iter()
            .flat_map(Option::into_iter)
            .collect::<TransactionBody>();
        Self::new(TransactionType::DownloadFile, body)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, From, Into)]
struct ForkCount(i16);

const FILP: &[u8; 4] = b"FILP";

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, From, Into)]
pub struct FlattenedFileHeader(ForkCount);

impl HotlineProtocol for FlattenedFileHeader {
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, _format) = bytes::streaming::tag(FILP)(bytes)?;
        let (bytes, _version) = verify(be_i16, |i: &i16| *i == 1,)(bytes)?;
        let (bytes, _reserved) = bytes::streaming::take(16usize)(bytes)?;
        let (bytes, fork_count) = map(be_i16, ForkCount::from)(bytes)?;
        let header = Self(fork_count);
        Ok((bytes, header))
    }
    fn into_bytes(self) -> Vec<u8> {
        let Self(fork_count) = self;
        [
            FILP.to_vec(),
            1i16.to_be_bytes().to_vec(),
            vec![0u8; 16],
            fork_count.0.to_be_bytes().to_vec(),
        ].concat()
    }
}

#[derive(From, Into)]
pub struct AsyncDataSource(u64, Box<dyn AsyncRead + Unpin + Send>);

pub struct FlattenedFileObject {
    pub version: crate::protocol::handshake::Version,
    pub info: InfoFork,
    pub contents: HashMap<ForkType, AsyncDataSource>,
}

impl FlattenedFileObject {
    pub fn with_data(info: InfoFork, data: AsyncDataSource) -> Self {
        Self {
            version: 1.into(),
            info,
            contents: hashmap! { ForkType::Data => data },
        }
    }
    pub fn with_forks(
        info: InfoFork,
        data: AsyncDataSource,
        rsrc: AsyncDataSource,
    ) -> Self {
        Self {
            version: 1.into(),
            info,
            contents: hashmap! {
                ForkType::Data => data,
                ForkType::Resource => rsrc,
            },
        }
    }
    pub fn header(&self) -> FlattenedFileHeader {
        let fork_count = (self.contents.len() + 1) as i16;
        FlattenedFileHeader(fork_count.into())
    }
    pub fn info(&self) -> (ForkHeader, InfoFork) {
        let data_size = (self.info.size() as i32).into();
        (
            ForkHeader {
                fork_type: ForkType::Info,
                compression_type: Default::default(),
                padding: [0u8; 4],
                data_size,
            },
            self.info.clone(),
        )
    }
    pub fn take_fork(&mut self, fork_type: ForkType) -> Option<(ForkHeader, AsyncDataSource)> {
        if let Some(fork) = self.contents.remove(&fork_type) {
            Some((
                ForkHeader {
                    fork_type,
                    compression_type: Default::default(),
                    padding: [0u8; 4],
                    data_size: (fork.0 as usize).into(),
                },
                fork,
            ))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, DekuRead, DekuWrite)]
#[deku(id_type = "u32")]
pub enum CompressionType {
    #[deku(id = "0u32")]
    None,
    #[deku(id_pat = "_")]
    Other(NonZeroU32),
}

impl Default for CompressionType {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, DekuRead, DekuWrite)]
#[deku(id_type = "[u8; 4]")]
pub enum PlatformType {
    #[deku(id = b"AMAC")]
    AppleMac,
    #[deku(id = b"MWIN")]
    MicrosoftWin,
    #[deku(id_pat = "_")]
    Other([u8; 4]),
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, DekuRead, DekuWrite)]
#[deku(id_type = "[u8; 4]")]
pub enum ForkType {
    #[deku(id = b"INFO")]
    Info,
    #[deku(id = b"DATA")]
    Data,
    #[deku(id = b"MACR")]
    Resource,
    #[deku(id_pat = "_")]
    Other([u8; 4]),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct FileFlags(i32);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct PlatformFlags(i32);

#[derive(Debug, Clone, DekuRead, DekuWrite)]
pub struct ForkHeader {
    pub fork_type: ForkType,
    pub compression_type: CompressionType,
    pub padding: [u8; 4],
    pub data_size: DataSize,
}

#[derive(Debug, Clone, DekuRead, DekuWrite)]
pub struct InfoFork {
    pub platform: PlatformType,
    pub type_code: FileType,
    pub creator_code: Creator,
    pub flags: FileFlags,
    pub platform_flags: PlatformFlags,
    pub padding: [u8; 4],
    pub created_at: FileCreatedAt,
    pub modified_at: FileModifiedAt,
    pub name_script: NameScript,
    #[deku(endian = "big")]
    pub name_len: i16,
    #[deku(count = "name_len")]
    pub file_name: Vec<u8>,
    #[deku(endian = "big")]
    pub comment_len: i16,
    #[deku(count = "comment_len")]
    pub comment: Vec<u8>,
}

impl InfoFork {
    pub fn size(&self) -> usize {
        74 + self.file_name.len() + self.comment.len()
    }
}

#[derive(Debug)]
pub struct UploadFile {
    pub filename: FileName,
    pub file_path: FilePath,
}

impl TryFrom<TransactionFrame> for UploadFile {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::UploadFile)?;

        let filename = body.require_field(TransactionField::FileName)
            .map(FileName::from)?;
        let file_path = body.borrow_field(TransactionField::FilePath)
            .try_into()?;

        Ok(Self { filename, file_path })
    }
}

impl From<UploadFile> for TransactionFrame {
    fn from(val: UploadFile) -> Self {
        let UploadFile { filename, file_path } = val;
        let body = [
            Some(filename.into()),
            file_path.into(),
        ]
            .into_iter()
            .flat_map(Option::into_iter)
            .collect::<TransactionBody>();
        Self::new(TransactionType::UploadFile, body)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UploadFileReply {
    pub reference: ReferenceNumber,
    // TODO: resume
}

impl TryFrom<TransactionFrame> for UploadFileReply {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame { body, ..  } = frame;
        let reference = body.require_field(TransactionField::ReferenceNumber)
            .and_then(ReferenceNumber::try_from)?;
        Ok(Self { reference })
    }
}

impl From<UploadFileReply> for TransactionFrame {
    fn from(val: UploadFileReply) -> Self {
        let UploadFileReply { reference } = val;
        let body = [reference.into()].into_iter()
            .collect::<TransactionBody>();
        Self::new(TransactionType::UploadFile, body)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ConnectionKeepAlive;

impl TryFrom<TransactionFrame> for ConnectionKeepAlive {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        frame.require_transaction_type(TransactionType::ConnectionKeepAlive)?;
        Ok(Self)
    }
}

impl From<ConnectionKeepAlive> for TransactionBody {
    fn from(_: ConnectionKeepAlive) -> Self {
        Default::default()
    }
}

#[derive(Debug, Clone)]
pub struct MoveFile {
    pub filename: FileName,
    pub path: FilePath,
    pub new_path: FilePath,
}

impl TryFrom<TransactionFrame> for MoveFile {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::MoveFile)?;
        let filename = body.require_field(TransactionField::FileName)
            .map(FileName::from)?;
        let path = body.borrow_field(TransactionField::FilePath)
            .try_into()
            .unwrap_or(FilePath::Root);
        let new_path = body.borrow_field(TransactionField::FileNewPath)
            .cloned()
            .map(Parameter::take)
            .and_then(|path| FilePath::try_from(path.as_slice()).ok())
            .unwrap_or(FilePath::Root);
        Ok(Self { filename, path, new_path })
    }
}

impl From<MoveFile> for TransactionFrame {
    fn from(val: MoveFile) -> Self {
        let MoveFile { filename, path, new_path } = val;
        let body = [
            Some(filename.into()),
            path.into(),
            new_path.into(),
        ]
            .into_iter()
            .flat_map(Option::into_iter)
            .collect::<TransactionBody>();
        Self::new(TransactionType::UploadFile, body)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MoveFileReply;

impl TryFrom<TransactionFrame> for MoveFileReply {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        frame.require_transaction_type(TransactionType::MoveFile)?;
        Ok(Self)
    }
}

impl From<MoveFileReply> for TransactionFrame {
    fn from(_: MoveFileReply) -> Self {
        Self::empty(TransactionType::MoveFile)
    }
}

#[derive(Debug, Clone)]
pub struct DeleteFile {
    pub filename: FileName,
    pub path: FilePath,
}

impl TryFrom<TransactionFrame> for DeleteFile {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::DeleteFile)?;
        let filename = body.require_field(TransactionField::FileName)
            .map(FileName::from)?;
        let path = body.borrow_field(TransactionField::FilePath)
            .try_into()
            .unwrap_or(FilePath::Root);
        Ok(Self { filename, path })
    }
}

impl From<DeleteFile> for TransactionFrame {
    fn from(val: DeleteFile) -> Self {
        let DeleteFile { filename, path } = val;
        let body = [
            Some(filename.into()),
            path.into(),
        ]
            .into_iter()
            .flat_map(Option::into_iter)
            .collect::<TransactionBody>();
        Self::new(TransactionType::DeleteFile, body)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeleteFileReply;

impl TryFrom<TransactionFrame> for DeleteFileReply {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        frame.require_transaction_type(TransactionType::DeleteFile)?;
        Ok(Self)
    }
}

impl From<DeleteFileReply> for TransactionFrame {
    fn from(_: DeleteFileReply) -> Self {
        Self::empty(TransactionType::DeleteFile)
    }
}

#[derive(Debug, Clone)]
pub struct NewFolder {
    pub filename: FileName,
    pub path: FilePath,
}

impl TryFrom<TransactionFrame> for NewFolder {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::NewFolder)?;
        let filename = body.require_field(TransactionField::FileName)
            .map(FileName::from)?;
        let path = body.borrow_field(TransactionField::FilePath)
            .try_into()
            .unwrap_or(FilePath::Root);
        Ok(Self { filename, path })
    }
}

impl From<NewFolder> for TransactionFrame {
    fn from(val: NewFolder) -> Self {
        let NewFolder { filename, path } = val;
        let body = [
            Some(filename.into()),
            path.into(),
        ]
            .into_iter()
            .flat_map(Option::into_iter)
            .collect::<TransactionBody>();
        Self::new(TransactionType::NewFolder, body)
    }
}

#[derive(Debug, Clone)]
pub struct MakeFileAlias {
    pub filename: FileName,
    pub source: FilePath,
    pub target: FilePath,
}

impl TryFrom<TransactionFrame> for MakeFileAlias {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::MakeFileAlias)?;

        let filename = body.require_field(TransactionField::FileName)
            .map(FileName::from)?;
        let source = body.borrow_field(TransactionField::FilePath)
            .try_into()
            .unwrap_or(FilePath::Root);
        let target = body.borrow_field(TransactionField::FileNewPath)
            .cloned()
            .map(Parameter::take)
            .and_then(|path| FilePath::try_from(path.as_slice()).ok())
            .unwrap_or(FilePath::Root);

        Ok(Self { filename, source, target })
    }
}

impl From<MakeFileAlias> for TransactionFrame {
    fn from(val: MakeFileAlias) -> Self {
        let MakeFileAlias { filename, source, target } = val;
        let body = [
            Some(filename.into()),
            source.into(),
            target.into(),
        ]
            .into_iter()
            .flat_map(Option::into_iter)
            .collect::<TransactionBody>();
        Self::new(TransactionType::MakeFileAlias, body)
    }
}

fn take_if_matches(
    parameter: Parameter,
    field: TransactionField,
) -> Result<Vec<u8>, ProtocolError> {
    if parameter.field_matches(field) {
        Ok(parameter.take())
    } else {
        Err(ProtocolError::UnexpectedTransaction {
            expected: field.into(),
            encountered: parameter.field_id.into(),
        })
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    static AUTHENTICATED_LOGIN: &[u8] = &[
        0x00, 0x00, 0x00, 0x6b, 0x00, 0x00, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x28,
        0x00, 0x00, 0x00, 0x28, 0x00, 0x04, 0x00, 0x69,
        0x00, 0x07, 0x95, 0x86, 0x9a, 0x93, 0x93, 0x90,
        0x85, 0x00, 0x6a, 0x00, 0x06, 0xce, 0xcd, 0xcc,
        0xcb, 0xca, 0xc9, 0x00, 0x66, 0x00, 0x07, 0x6a,
        0x79, 0x65, 0x6c, 0x6c, 0x6f, 0x7a, 0x00, 0x68,
        0x00, 0x02, 0x00, 0x91,
    ];

    #[test]
    fn parse_authenticated_login() {

        let (tail, frame) = <TransactionFrame as HotlineProtocol>::from_bytes(AUTHENTICATED_LOGIN)
            .expect("could not parse valid login packet");

        assert!(tail.is_empty());

        let login = LoginRequest::try_from(frame)
            .expect("could not view transaction as login request");

        assert_eq!(
            login,
            LoginRequest {
                login: Some(UserLogin::from_cleartext(b"jyelloz")),
                nickname: Some(Nickname::new((*b"jyelloz").into())),
                password: Some(Password::from_cleartext(b"123456")),
                icon_id: Some(145.into()),
            },
        );

    }

}
