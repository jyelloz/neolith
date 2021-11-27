use nom::{
    self,
    IResult,
    multi,
    bytes::{
        self,
        streaming::take,
    },
    number::streaming::{
        be_i16,
        be_i32,
        be_i8,
    },
};

use derive_more::{From, Into};

use thiserror::Error;

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

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
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

impl HotlineProtocol for ErrorCode {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i32(bytes)?;
        Ok((bytes, Self(value)))
    }
}

pub use handshake::{
    ClientHandshakeRequest,
    ServerHandshakeReply,
    SubProtocolId,
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
    UserFlags,
    UserId,
    UserLogin,
    UserNameWithInfo,
};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LoginRequest {
    pub login: Option<UserLogin>,
    pub nickname: Option<Nickname>,
    pub password: Option<Password>,
    pub icon_id: Option<IconId>,
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

impl Into<TransactionFrame> for LoginReply {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::Login.into(),
            ..Default::default()
        };
        let Self(version) = self;
        let parameters = vec![version.into()];
        let body = TransactionBody { parameters };
        TransactionFrame { header, body }
    }
}

#[derive(Debug, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProtocolVersion(i16);

impl Into<Parameter> for ProtocolVersion {
    fn into(self) -> Parameter {
        Parameter::new_int(
            TransactionField::Version.into(),
            self.0.into(),
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

impl Into<Parameter> for ServerAgreement {
    fn into(self) -> Parameter {
        Parameter::new(
            TransactionField::Data.into(),
            self.0,
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
    URL,
    Data,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ServerBanner {
    URL(Vec<u8>),
    Data(Vec<u8>),
}

impl TryFrom<TransactionFrame> for LoginRequest {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {

        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::Login)?;

        let TransactionBody { parameters, .. } = body;

        let login = parameters.iter()
            .find_map(|p| UserLogin::try_from(p).ok());

        let nickname = parameters.iter()
            .find_map(|p| Nickname::try_from(p).ok());

        let password = parameters.iter()
            .find_map(|p| Password::try_from(p).ok());

        let icon_id = parameters.iter()
            .find_map(|p| IconId::try_from(p).ok());

        Ok(Self { login, nickname, password, icon_id })
    }
}

impl TryFrom<TransactionBody> for ShowAgreement {
    type Error = ProtocolError;
    fn try_from(body: TransactionBody) -> Result<Self, Self::Error> {

        let TransactionBody { parameters, .. } = body;

        let agreement = parameters.iter()
            .find_map(|p| ServerAgreement::try_from(p).ok());

        let no_agreement = parameters.iter()
            .any(|p| p.field_matches(TransactionField::NoServerAgreement));

        let agreement = if no_agreement {
            None
        } else {
            agreement
        };

        let banner_type = parameters.iter()
            .filter(|p| p.field_matches(TransactionField::ServerBannerType))
            .map(ServerBannerType::try_from)
            .next();

        let banner = None;

        Ok(Self { agreement, banner })
    }
}

impl TryFrom<&Parameter> for ServerBannerType {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let field_data = parameter.borrow();
        match &field_data[..] {
            &[1] => {
                Ok(ServerBannerType::URL)
            },
            &[0] => {
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

impl Into<TransactionBody> for LoginRequest {
    fn into(self) -> TransactionBody {

        let Self { login, nickname, password, icon_id } = self;

        let login = login.map(UserLogin::into);
        let password = password.map(Password::into);
        let nickname = nickname.map(Nickname::into);
        let icon_id = icon_id.map(IconId::into);

        let parameters = [login, nickname, password, icon_id].into_iter()
            .flat_map(Option::into_iter)
            .collect();

        TransactionBody { parameters }
    }
}

impl Into<TransactionBody> for ShowAgreement {
    fn into(self) -> TransactionBody {
        let parameter = if let Some(agreement) = self.agreement {
            agreement.into()
        } else {
            Parameter::new_int(
                TransactionField::NoServerAgreement.into(),
                1i16.into(),
            )
        };
        TransactionBody { parameters: vec![parameter] }
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

        let TransactionBody { parameters, .. } = body;

        let username = parameters.iter()
            .find_map(|p| Nickname::try_from(p).ok())
            .ok_or(ProtocolError::MissingField(TransactionField::UserName))?;

        let icon_id = parameters.iter()
            .find_map(|p| IconId::try_from(p).ok())
            .ok_or(ProtocolError::MissingField(TransactionField::UserIconId))?;

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

impl Into<TransactionFrame> for NotifyUserChange {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::NotifyUserChange.into(),
            ..Default::default()
        };
        let Self {
            user_id,
            username,
            icon_id,
            user_flags,
        } = self;
        let body = vec![
            user_id.into(),
            icon_id.into(),
            user_flags.into(),
            username.into(),
        ].into();
        TransactionFrame { header, body }
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

impl Into<TransactionFrame> for NotifyUserDelete {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::NotifyUserDelete.into(),
            ..Default::default()
        };
        let Self { user_id } = self;
        let body = vec![user_id.into()].into();
        TransactionFrame { header, body }
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

impl Into<TransactionFrame> for NotifyChatUserChange {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::NotifyChatUserChange.into(),
            ..Default::default()
        };
        let Self {
            chat_id,
            user_id,
            icon_id,
            user_flags,
            user_name,
        } = self;
        let body = vec![
            chat_id.into(),
            user_id.into(),
            icon_id.into(),
            user_flags.into(),
            user_name.into(),
        ].into();
        TransactionFrame { header, body }
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

impl Into<TransactionFrame> for NotifyChatUserDelete {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::NotifyChatUserDelete.into(),
            ..Default::default()
        };
        let Self {
            chat_id,
            user_id,
            icon_id,
            user_flags,
            user_name,
        } = self;
        let body = vec![
            chat_id.into(),
            user_id.into(),
            icon_id.into(),
            user_flags.into(),
            user_name.into(),
        ].into();
        TransactionFrame { header, body }
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

impl Into<TransactionFrame> for NotifyChatSubject {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::NotifyChatSubject.into(),
            ..Default::default()
        };
        let Self { chat_id, subject } = self;
        let body = vec![
            chat_id.into(),
            subject.into(),
        ].into();
        TransactionFrame { header, body }
    }
}

impl TryFrom<TransactionFrame> for NotifyChatSubject {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let TransactionFrame { body, .. } = frame;
        let TransactionBody { parameters } = body;
        let chat_id = parameters.iter()
            .find(|p| p.field_matches(TransactionField::ChatId))
            .ok_or(ProtocolError::MissingField(TransactionField::ChatId))
            .and_then(|p| ChatId::try_from(p))?;
        let subject = parameters.iter()
            .find(|p| p.field_matches(TransactionField::ChatSubject))
            .ok_or(ProtocolError::MissingField(TransactionField::ChatSubject))
            .and_then(|p| ChatSubject::try_from(p))?;
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

impl Into<TransactionBody> for GetUserNameList {
    fn into(self) -> TransactionBody {
        Default::default()
    }
}

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

impl Into<TransactionFrame> for GetUserNameListReply {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::GetUserNameList.into(),
            is_reply: IsReply::reply(),
            ..Default::default()
        };
        let Self(users) = self;
        let body = users.into_iter()
            .map(UserNameWithInfo::into)
            .collect();
        TransactionFrame { header, body }
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

impl Into<TransactionFrame> for GetMessages {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::GetMessages.into(),
            ..Default::default()
        };
        TransactionFrame::empty(header)
    }
}

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

impl Into<TransactionFrame> for GetMessagesReply {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::GetMessages.into(),
            is_reply: IsReply::reply(),
            ..Default::default()
        };
        let Self(messages) = self;
        let body = messages.into_iter()
            .map(Message::into)
            .collect();
        TransactionFrame { header, body }
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

        let TransactionBody { parameters, .. } = body;

        let path = parameters.iter()
            .find_map(|p| FilePath::try_from(p).ok())
            .unwrap_or_default();

        Ok(Self(path))
    }
}

impl Into<TransactionFrame> for GetFileNameList {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::GetFileNameList.into(),
            ..Default::default()
        };
        TransactionFrame::empty(header)
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

impl Into<TransactionFrame> for GetFileNameListReply {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::GetFileNameList.into(),
            is_reply: IsReply::reply(),
            ..Default::default()
        };
        let body = self.0.into_iter()
            .map(FileNameWithInfo::into)
            .collect();
        TransactionFrame { header, body }
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

impl Into<Parameter> for FileNameWithInfo {
    fn into(self) -> Parameter {
        let filename_size = self.file_name.len() as i16;
        let data = [
            &self.file_type.0[..],
            &self.creator.0[..],
            &(i32::from(self.file_size)).to_be_bytes()[..],
            &[0u8; 4][..],
            &self.name_script.0.to_be_bytes()[..],
            &filename_size.to_be_bytes()[..],
            &self.file_name[..],
        ].into_iter()
            .flat_map(|bytes| bytes.into_iter())
            .map(|b| *b)
            .collect();
        Parameter::new(
            TransactionField::FileNameWithInfo.into(),
            data,
        )
    }
}

#[derive(Debug, From, Into)]
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

        let TransactionBody { parameters, .. } = body;

        let filename = parameters.iter()
            .find(|p| p.field_matches(TransactionField::FileName))
            .ok_or(ProtocolError::MissingField(TransactionField::FileName))
            .map(FileName::from)?;

        let path = parameters.iter()
            .find(|p| p.field_matches(TransactionField::FilePath))
            .map(|p| FilePath::try_from(p));

        let path = if let Some(path) = path {
            path?
        } else {
            FilePath::Root
        };

        Ok(Self { filename, path })
    }
}

impl Into<TransactionFrame> for GetFileInfo {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::GetFileNameList.into(),
            ..Default::default()
        };
        TransactionFrame::empty(header)
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

impl Into<TransactionFrame> for GetFileInfoReply {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::GetFileInfo.into(),
            ..Default::default()
        };
        let type_string = FileTypeString::from(&self.type_code);
        let body = vec![
            self.filename.into(),
            self.type_code.into(),
            self.creator.into(),
            self.created_at.into(),
            self.modified_at.into(),
            self.comment.into(),
            self.size.into(),
            type_string.into(),
        ].into();
        TransactionFrame { header, body }
    }
}

#[derive(Debug)]
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

        let TransactionBody { parameters, .. } = body;

        let options = parameters.iter()
            .filter(|p| p.field_matches(TransactionField::ChatOptions))
            .find_map(|p| ChatOptions::try_from(p).ok())
            .unwrap_or_default();

        let chat_id = parameters.iter()
            .filter(|p| p.field_matches(TransactionField::ChatId))
            .find_map(|p| ChatId::try_from(p).ok());

        let message = parameters.into_iter()
            .filter(|p| p.field_matches(TransactionField::Data))
            .map(|p| p.take())
            .next()
            .ok_or(ProtocolError::MissingField(TransactionField::Data))?;

        let chat = Self {
            options,
            chat_id,
            message,
        };

        Ok(chat)
    }
}

impl Into<TransactionFrame> for SendChat {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::SendChat.into(),
            ..Default::default()
        };
        let Self { message, chat_id, options } = self;
        let body = vec![
            Some(Parameter::new(TransactionField::Data.into(), message)),
            chat_id.map(ChatId::into),
            Some(options.into()),
        ].into_iter()
            .flat_map(Option::into_iter)
            .collect();
        TransactionFrame { header, body }
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

        let TransactionBody { parameters, .. } = body;

        let chat_id = parameters.iter()
            .filter(|p| p.field_matches(TransactionField::ChatId))
            .find_map(|p| ChatId::try_from(p).ok());

        let message = parameters.into_iter()
            .filter(|p| p.field_matches(TransactionField::Data))
            .map(|p| p.take())
            .next()
            .ok_or(ProtocolError::MissingField(TransactionField::Data))?;

        Ok(Self { chat_id, message })
    }
}

impl Into<TransactionFrame> for ChatMessage {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::ChatMessage.into(),
            ..Default::default()
        };
        let Self { message, chat_id } = self;
        let message = Parameter::new(
            TransactionField::Data.into(),
            message,
        );
        let chat_id = chat_id.map(ChatId::into);
        let body = vec![
            Some(message),
            chat_id,
        ].into_iter()
            .flat_map(Option::into_iter)
            .collect();
        TransactionFrame { header, body }
    }
}

#[derive(Debug)]
pub struct ServerMessage {
    pub user_id: Option<UserId>,
    pub user_name: Option<Nickname>,
    pub message: Vec<u8>,
}

impl Into<TransactionFrame> for ServerMessage {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::ServerMessage.into(),
            ..Default::default()
        };
        let Self { message, user_id, user_name } = self;
        let message = Parameter::new(
            TransactionField::Data.into(),
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
        TransactionFrame { header, body }
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
        let TransactionBody { parameters, .. } = body;
        let message = parameters.into_iter()
            .filter(|p| p.field_matches(TransactionField::Data))
            .map(|p| p.take())
            .next()
            .ok_or(ProtocolError::MissingField(TransactionField::Data))?;
        Ok(Self { message })
    }
}

impl Into<TransactionFrame> for DisconnectMessage {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::DisconnectMessage.into(),
            ..Default::default()
        };
        let Self { message } = self;
        let message = Parameter::new(
            TransactionField::Data.into(),
            message,
        );
        let body = vec![message].into();
        TransactionFrame { header, body }
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

        let TransactionBody { parameters, .. } = body;

        let user_id = parameters.iter()
            .find(|p| p.field_matches(TransactionField::UserId))
            .ok_or(ProtocolError::MissingField(TransactionField::UserId))
            .and_then(UserId::try_from)?;

        let message = parameters.into_iter()
            .filter(|p| p.field_matches(TransactionField::Data))
            .map(|p| p.take())
            .next()
            .ok_or(ProtocolError::MissingField(TransactionField::Data))?;

        Ok(Self { user_id, message })
    }
}

impl Into<TransactionFrame> for SendInstantMessage {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::SendChat.into(),
            ..Default::default()
        };
        let Self { user_id, message } = self;
        let body = vec![
            user_id.into(),
            Parameter::new(TransactionField::Data.into(), message),
        ].into();
        TransactionFrame { header, body }
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

impl Into<TransactionFrame> for SendInstantMessageReply {
    fn into(self) -> TransactionFrame {
        TransactionFrame::empty(Default::default())
    }
}

#[derive(Debug, From, Into)]
pub struct InviteToNewChat(Vec<UserId>);

impl TryFrom<TransactionFrame> for InviteToNewChat {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let frame = frame.require_transaction_type(TransactionType::InviteToNewChat)?;
        let TransactionFrame { body, .. } = frame;
        let TransactionBody { parameters } = body;

        let user_ids: Result<_, _> = parameters.iter()
            .filter(|p| p.field_matches(TransactionField::UserId))
            .map(UserId::try_from)
            .collect();

        Ok(Self(user_ids?))
    }
}

impl Into<TransactionFrame> for InviteToNewChat {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::InviteToNewChat.into(),
            ..Default::default()
        };
        let Self(user_ids) = self;
        let body = user_ids.into_iter()
            .map(UserId::into)
            .collect();
        TransactionFrame { header, body }
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
        let TransactionBody { parameters } = body;

        let chat_id = parameters.iter()
            .find(|p| p.field_matches(TransactionField::ChatId))
            .ok_or(ProtocolError::MissingField(TransactionField::ChatId))
            .and_then(ChatId::try_from)?;

        let user_id = parameters.iter()
            .find(|p| p.field_matches(TransactionField::UserId))
            .ok_or(ProtocolError::MissingField(TransactionField::UserId))
            .and_then(UserId::try_from)?;

        let icon_id = parameters.iter()
            .find(|p| p.field_matches(TransactionField::UserIconId))
            .ok_or(ProtocolError::MissingField(TransactionField::UserIconId))
            .and_then(IconId::try_from)?;

        let flags = parameters.iter()
            .find(|p| p.field_matches(TransactionField::UserFlags))
            .ok_or(ProtocolError::MissingField(TransactionField::UserFlags))
            .and_then(UserFlags::try_from)?;

        let user_name = parameters.iter()
            .find(|p| p.field_matches(TransactionField::UserName))
            .ok_or(ProtocolError::MissingField(TransactionField::UserName))
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

impl Into<TransactionFrame> for InviteToNewChatReply {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::InviteToNewChat.into(),
            ..Default::default()
        };
        let Self {
            chat_id,
            user_id,
            user_name,
            icon_id,
            flags,
        } = self;
        let body = vec![
            chat_id.into(),
            user_id.into(),
            icon_id.into(),
            user_name.into(),
            flags.into(),
        ].into();
        TransactionFrame { header, body }
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
        let TransactionBody { parameters } = body;

        let user_id = parameters.iter()
            .find(|p| p.field_matches(TransactionField::UserId))
            .ok_or(ProtocolError::MissingField(TransactionField::UserId))
            .and_then(UserId::try_from)?;

        let chat_id = parameters.iter()
            .find(|p| p.field_matches(TransactionField::ChatId))
            .ok_or(ProtocolError::MissingField(TransactionField::ChatId))
            .and_then(ChatId::try_from)?;

        Ok(Self { user_id, chat_id })
    }
}

impl Into<TransactionFrame> for InviteToChat {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::InviteToChat.into(),
            ..Default::default()
        };
        let Self { user_id, chat_id } = self;
        let body = vec![
            user_id.into(),
            chat_id.into(),
        ].into();
        TransactionFrame { header, body }
    }
}

#[derive(Debug, From, Into)]
pub struct JoinChat(ChatId);

impl TryFrom<TransactionFrame> for JoinChat {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let frame = frame.require_transaction_type(TransactionType::JoinChat)?;
        let TransactionFrame { body, .. } = frame;
        let TransactionBody { parameters } = body;

        let chat_id = parameters.iter()
            .find(|p| p.field_matches(TransactionField::ChatId))
            .ok_or(ProtocolError::MissingField(TransactionField::ChatId))
            .and_then(ChatId::try_from)?;

        Ok(Self(chat_id))
    }
}

impl Into<TransactionFrame> for JoinChat {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::JoinChat.into(),
            ..Default::default()
        };
        let Self(chat_id) = self;
        let body = vec![chat_id.into()].into();
        TransactionFrame { header, body }
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
        let TransactionBody { parameters } = body;

        let subject = parameters.iter()
            .find(|p| p.field_matches(TransactionField::ChatSubject))
            .and_then(|p| ChatSubject::try_from(p).ok());

        let users = parameters.iter()
            .filter(|p| p.field_matches(TransactionField::UserNameWithInfo))
            .map(UserNameWithInfo::try_from)
            .collect::<Result<_, _>>()?;

        Ok(Self { subject, users })
    }
}

impl Into<TransactionFrame> for JoinChatReply {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::Reply.into(),
            ..Default::default()
        };
        let Self { subject, users } = self;
        let subject = subject.map(ChatSubject::into);
        let users: Vec<Parameter> = users.into_iter()
            .map(UserNameWithInfo::into)
            .collect();
        let body = subject.into_iter()
            .chain(users.into_iter())
            .collect();
        TransactionFrame { header, body }
    }
}

#[derive(Debug, From, Into)]
pub struct LeaveChat(ChatId);

impl TryFrom<TransactionFrame> for LeaveChat {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let frame = frame.require_transaction_type(TransactionType::LeaveChat)?;
        let TransactionFrame { body, .. } = frame;
        let TransactionBody { parameters } = body;

        let chat_id = parameters.iter()
            .find(|p| p.field_matches(TransactionField::ChatId))
            .ok_or(ProtocolError::MissingField(TransactionField::ChatId))
            .and_then(ChatId::try_from)?;

        Ok(Self(chat_id))
    }
}

impl Into<TransactionFrame> for LeaveChat {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::LeaveChat.into(),
            ..Default::default()
        };
        let Self(chat_id) = self;
        let body = vec![chat_id.into()].into();
        TransactionFrame { header, body }
    }
}

#[derive(Debug, From, Into)]
pub struct SetChatSubject(ChatId, ChatSubject);

impl TryFrom<TransactionFrame> for SetChatSubject {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        let frame = frame.require_transaction_type(TransactionType::SetChatSubject)?;
        let TransactionFrame { body, .. } = frame;
        let TransactionBody { parameters } = body;

        let chat_id = parameters.iter()
            .find(|p| p.field_matches(TransactionField::ChatId))
            .ok_or(ProtocolError::MissingField(TransactionField::ChatId))
            .and_then(ChatId::try_from)?;

        let subject = parameters.iter()
            .find(|p| p.field_matches(TransactionField::ChatSubject))
            .ok_or(ProtocolError::MissingField(TransactionField::ChatSubject))
            .and_then(ChatSubject::try_from)?;

        Ok(Self(chat_id, subject))
    }
}

impl Into<TransactionFrame> for SetChatSubject {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::SetChatSubject.into(),
            ..Default::default()
        };
        let Self(chat_id, subject) = self;
        let body = vec![
            chat_id.into(),
            subject.into(),
        ].into();
        TransactionFrame { header, body }
    }
}

#[derive(Debug)]
pub struct GetClientInfoTextRequest {
    pub user_id: UserId,
}

impl TryFrom<TransactionFrame> for GetClientInfoTextRequest {
    type Error = ProtocolError;
    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {

        let TransactionFrame {
            body, ..
        } = frame.require_transaction_type(TransactionType::GetClientInfoText)?;

        let TransactionBody { parameters, .. } = body;

        let user_id = parameters.iter()
            .find(|p| p.field_matches(TransactionField::UserId))
            .ok_or(ProtocolError::MissingField(TransactionField::UserId))
            .and_then(UserId::try_from)?;

        Ok(Self { user_id })
    }
}

impl Into<TransactionFrame> for GetClientInfoTextRequest {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::GetClientInfoText.into(),
            ..Default::default()
        };
        let Self { user_id, .. } = self;
        let body = vec![user_id.into()].into();
        TransactionFrame { header, body }
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

        let TransactionBody { parameters, .. } = body;

        let user_name = parameters.iter()
            .find(|p| p.field_matches(TransactionField::UserName))
            .ok_or(ProtocolError::MissingField(TransactionField::UserName))
            .and_then(Nickname::try_from)?;

        let text = parameters.iter()
            .cloned()
            .find(|p| p.field_matches(TransactionField::Data))
            .map(Parameter::take)
            .ok_or(ProtocolError::MissingField(TransactionField::Data))?;

        Ok(Self { user_name, text })
    }
}

impl Into<TransactionFrame> for GetClientInfoTextReply {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::GetClientInfoText.into(),
            ..Default::default()
        };
        let Self { user_name, text } = self;
        let body = vec![
            user_name.into(),
            Parameter::new(TransactionField::Data.into(), text),
        ].into();
        TransactionFrame { header, body }
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

        let TransactionBody { parameters, .. } = body;

        let message = parameters.into_iter()
            .filter(|p| p.field_matches(TransactionField::Data))
            .map(|p| p.take())
            .next()
            .ok_or(ProtocolError::MissingField(TransactionField::Data))?;

        Ok(Self { message })
    }
}

impl Into<TransactionFrame> for SendBroadcast {
    fn into(self) -> TransactionFrame {
        let header = TransactionHeader {
            _type: TransactionType::GetClientInfoText.into(),
            ..Default::default()
        };
        let Self { message } = self;
        let body = vec![
            Parameter::new(TransactionField::Data.into(), message),
        ].into();
        TransactionFrame { header, body }
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

impl Into<TransactionFrame> for GenericReply {
    fn into(self) -> TransactionFrame {
        TransactionFrame::empty(Default::default())
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

    static AUTHENTICATED_LOGIN: &'static [u8] = &[
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

        let (tail, frame) = TransactionFrame::from_bytes(AUTHENTICATED_LOGIN)
            .expect("could not parse valid login packet");

        assert!(tail.is_empty());

        let login = LoginRequest::try_from(frame)
            .expect("could not view transaction as login request");

        assert_eq!(
            login,
            LoginRequest {
                login: Some(UserLogin::from_cleartext(b"jyelloz")),
                nickname: Some(Nickname::new(b"jyelloz".clone().into())),
                password: Some(Password::from_cleartext(b"123456")),
                icon_id: Some(145.into()),
            },
        );

    }

}
