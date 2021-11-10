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
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct ErrorCode(i32);

impl ErrorCode {
    pub fn ok() -> Self {
        Self(0)
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
};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LoginRequest {
    pub login: Option<UserLogin>,
    pub nickname: Nickname,
    pub password: Option<Password>,
    pub icon_id: IconId,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UserLogin(Vec<u8>);

impl UserLogin {
    pub fn new(login: Vec<u8>) -> Self {
        Self(login)
    }
    pub fn from_cleartext(clear: &[u8]) -> Self {
        Self(invert_credential(clear))
    }
    pub fn raw_data(&self) -> &[u8] {
        &self.0
    }
    pub fn take(self) -> Vec<u8> {
        self.0
    }
}

impl Credential for UserLogin {
    fn deobfuscate(&self) -> Vec<u8> {
        invert_credential(&self.0)
    }
}

impl TryFrom<&Parameter> for UserLogin {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let data = take_if_matches(
            parameter.clone(),
            TransactionField::UserLogin,
        )?;
        Ok(Self::new(data))
    }
}

impl Into<Parameter> for UserLogin {
    fn into(self) -> Parameter {
        let Self(field_data) = self;
        let field_id = FieldId::from(TransactionField::UserLogin);
        Parameter::new(field_id, field_data)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Nickname(Vec<u8>);

impl Nickname {
    pub fn new(nickname: Vec<u8>) -> Self {
        Self(nickname)
    }
    pub fn take(self) -> Vec<u8> {
        self.0
    }
}

impl Default for Nickname {
    fn default() -> Self {
        Self(b"unnamed".to_vec())
    }
}

impl TryFrom<&Parameter> for Nickname {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let data = take_if_matches(
            parameter.clone(),
            TransactionField::UserName,
        )?;
        Ok(Self::new(data))
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Password(Vec<u8>);

impl TryFrom<&Parameter> for Password {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let data = take_if_matches(
            parameter.clone(),
            TransactionField::UserPassword,
        )?;
        Ok(Self::new(data))
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
        Parameter::new(
            TransactionField::Version.into(),
            self.0.to_be_bytes().to_vec(),
        )
    }
}

#[derive(Debug, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord)]
pub struct IconId(i16);

impl Into<Parameter> for IconId {
    fn into(self) -> Parameter {
        Parameter::new(
            TransactionField::UserIconId.into(),
            self.0.to_be_bytes().to_vec(),
        )
    }
}

impl TryFrom<&Parameter> for IconId {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let data = take_if_matches(
            parameter.clone(),
            TransactionField::UserIconId,
        )?;
        let result: BIResult<i16> = be_i16(&data[..]);
        match result {
            Ok((_, data)) => Ok(data.into()),
            Err(_) => Err(
                ProtocolError::MalformedData(TransactionField::UserIconId)
            ),
        }
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

fn invert_credential(data: &[u8]) -> Vec<u8> {
    data.iter()
        .map(|b| !b)
        .collect()
}

trait Credential {
    fn deobfuscate(&self) -> Vec<u8>;
}

impl Password {
    pub fn new(password: Vec<u8>) -> Self {
        Self(password)
    }
    pub fn from_cleartext(clear: &[u8]) -> Self {
        Self(invert_credential(clear))
    }
    pub fn raw_data(&self) -> &[u8] {
        &self.0
    }
    pub fn take(self) -> Vec<u8> {
        self.0
    }
}

impl Credential for Password {
    fn deobfuscate(&self) -> Vec<u8> {
        invert_credential(&self.0)
    }
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
            .find_map(|p| Nickname::try_from(p).ok())
            .ok_or(ProtocolError::MissingField(TransactionField::UserName))?;

        let password = parameters.iter()
            .find_map(|p| Password::try_from(p).ok());

        let icon_id = parameters.iter()
            .find_map(|p| IconId::try_from(p).ok())
            .ok_or(ProtocolError::MissingField(TransactionField::UserIconId))?;

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

impl Into<Parameter> for Nickname {
    fn into(self) -> Parameter {
        let Self(nickname) = self;
        Parameter::new(
            TransactionField::UserName.into(),
            nickname,
        )
    }
}

impl Into<Parameter> for Password {
    fn into(self) -> Parameter {
        let Self(password) = self;
        Parameter::new(
            TransactionField::UserPassword.into(),
            password,
        )
    }
}

impl Into<TransactionBody> for LoginRequest {
    fn into(self) -> TransactionBody {

        let Self { login, nickname, password, icon_id } = self;

        let login = login.map(UserLogin::into);
        let password = password.map(Password::into);
        let nickname = Some(nickname.into());
        let icon_id = Some(icon_id.into());

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
            Parameter::new(
                TransactionField::NoServerAgreement.into(),
                1i16.to_be_bytes().to_vec(),
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
        let parameters: Vec<Parameter> = users.into_iter()
            .map(UserNameWithInfo::into)
            .collect();
        TransactionFrame { header, body: parameters.into() }
    }
}

#[derive(Debug, Clone)]
pub struct UserNameWithInfo {
    pub user_id: UserId,
    pub icon_id: IconId,
    pub user_flags: UserFlags,
    pub username: Nickname,
}

impl Into<Parameter> for UserNameWithInfo {
    fn into(self) -> Parameter {
        let username = self.username.take();
        let username_len = username.len() as i16;
        let data = [
            &self.user_id.0.to_be_bytes()[..],
            &self.icon_id.0.to_be_bytes()[..],
            &self.user_flags.0.to_be_bytes()[..],
            &username_len.to_be_bytes()[..],
            &username[..],
        ].into_iter()
            .flat_map(|bytes| bytes.into_iter())
            .map(|b| *b)
            .collect();
        Parameter::new(
            TransactionField::UserNameWithInfo.into(),
            data,
        )
    }
}

#[derive(Debug, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord)]
pub struct UserId(i16);

#[derive(Debug, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord)]
pub struct UserFlags(i16);

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
        let parameters: Vec<Parameter> = self.0.into_iter()
            .map(|message| message.into())
            .collect();
        TransactionFrame { header, body: parameters.into() }
    }
}

pub struct Message(Vec<u8>);

impl Message {
    pub fn new(message: Vec<u8>) -> Self {
        Self(message)
    }
}

impl Into<Parameter> for Message {
    fn into(self) -> Parameter {
        Parameter::new(
            TransactionField::Data.into(),
            self.0,
        )
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
pub enum FilePath {
    Root,
    Directory(Vec<Vec<u8>>),
}
impl FilePath {
    pub fn path(&self) -> Option<&[Vec<u8>]> {
        if let Self::Directory(path) = self {
            Some(path)
        } else {
            None
        }
    }
    fn parse_depth(bytes: &[u8]) -> BIResult<usize> {
        let (bytes, depth) = be_i16(bytes)?;
        Ok((bytes, depth as usize))
    }
    fn parse_path_component(bytes: &[u8]) -> BIResult<&[u8]> {
        let (bytes, _) = take(2usize)(bytes)?;
        let (bytes, length) = be_i8(bytes)?;
        let (bytes, name) = take(length as usize)(bytes)?;
        Ok((bytes, name))
    }
    fn parse_path(bytes: &[u8]) -> BIResult<Vec<&[u8]>> {
        let (bytes, depth) = Self::parse_depth(bytes)?;
        multi::count(Self::parse_path_component, depth)(bytes)
    }
}

impl Default for FilePath {
    fn default() -> Self {
        Self::Root
    }
}

impl TryFrom<&[u8]> for FilePath {
    type Error = ProtocolError;
    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        match Self::parse_path(bytes) {
            Ok((_, components)) => {
                let components = components.iter()
                    .map(|c| c.to_vec())
                    .collect();
                Ok(Self::Directory(components))
            },
            Err(_) => Err(ProtocolError::MalformedData(TransactionField::FilePath))
        }
    }
}

impl TryFrom<&Parameter> for FilePath {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let data = take_if_matches(parameter.clone(), TransactionField::FilePath)?;
        Self::try_from(data.as_slice())
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
        let parameters: Vec::<Parameter> = self.0.into_iter()
            .map(FileNameWithInfo::into)
            .collect();
        TransactionFrame { header, body: parameters.into() }
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
            &self.file_size.0.to_be_bytes()[..],
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
pub struct FileType([u8; 4]);

impl From<&[u8; 4]> for FileType {
    fn from(bytes: &[u8; 4]) -> Self {
        (*bytes).into()
    }
}

#[derive(Debug, From, Into)]
pub struct Creator([u8; 4]);

impl From<&[u8; 4]> for Creator {
    fn from(bytes: &[u8; 4]) -> Self {
        (*bytes).into()
    }
}

#[derive(Debug, From, Into)]
pub struct FileSize(i32);

#[derive(Debug, From, Into)]
pub struct NameScript(i16);

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
                nickname: Nickname::new(b"jyelloz".clone().into()),
                password: Some(Password::from_cleartext(b"123456")),
                icon_id: 145.into(),
            },
        );

    }

}
