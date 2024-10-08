use super::{
    HotlineProtocol,
    ProtocolError,
    transaction::Parameter,
    transaction_field::TransactionField,
    multi,
    take,
    be_i8,
    be_i16,
    BIResult,
    date::DateParameter,
};
use derive_more::{From, Into};
use encoding_rs::MACINTOSH;
use std::{
    time::SystemTime,
    fmt::{Debug, Formatter, self},
    path::PathBuf,
};
use deku::prelude::*;

pub trait Credential {
    fn deobfuscate(&self) -> Vec<u8>;
}

fn invert_credential(data: &[u8]) -> Vec<u8> {
    data.iter()
        .map(|b| !b)
        .collect()
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, From, DekuRead, DekuWrite)]
#[deku(ctx = "len: usize", ctx_default = "0")]
pub struct Nickname(#[deku(count = "len")] Vec<u8>);

impl Nickname {
    pub fn new(nickname: Vec<u8>) -> Self {
        Self(nickname)
    }
    pub fn take(self) -> Vec<u8> {
        self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl std::fmt::Display for Nickname {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let (text, _, _) = MACINTOSH.decode(&self.0);
        f.write_str(&text)
    }
}

impl Debug for Nickname {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (text, _, _) = MACINTOSH.decode(&self.0);
        f.debug_tuple("Nickname")
            .field(&text)
            .finish()
    }
}

impl Default for Nickname {
    fn default() -> Self {
        Self(b"unnamed".to_vec())
    }
}

impl From<String> for Nickname {
    fn from(s: String) -> Self {
        s.into_bytes().into()
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

impl From<Nickname> for Parameter {
    fn from(val: Nickname) -> Self {
        Parameter::new(TransactionField::UserName, val.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UserLogin(Vec<u8>);

impl UserLogin {
    pub fn new(login: Vec<u8>) -> Self {
        Self(login)
    }
    pub fn from_cleartext(clear: &[u8]) -> Self {
        Self(invert_credential(clear))
    }
    pub fn invert(mut self) -> Self {
        self.0 = invert_credential(&self.0);
        self
    }
    pub fn raw_data(&self) -> &[u8] {
        &self.0
    }
    pub fn take(self) -> Vec<u8> {
        self.0
    }
    pub fn text(&self) -> String {
        let (text, _, _) = MACINTOSH.decode(&self.0);
        text.to_string()
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

impl From<UserLogin> for Parameter {
    fn from(val: UserLogin) -> Self {
        Parameter::new(TransactionField::UserLogin, val.0)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Password(Vec<u8>);

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

impl From<Password> for Parameter {
    fn from(val: Password) -> Self {
        Parameter::new(TransactionField::UserPassword, val.0)
    }
}

impl Credential for Password {
    fn deobfuscate(&self) -> Vec<u8> {
        invert_credential(&self.0)
    }
}

#[derive(Debug, Default, From, Into, PartialEq, Eq, PartialOrd, Ord, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct UserAccess(i64);

impl TryFrom<&Parameter> for UserAccess {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.read_deku()
            .map_err(|_| ProtocolError::MalformedData(TransactionField::UserAccess))
    }
}

impl From<UserAccess> for Parameter {
    fn from(val: UserAccess) -> Self {
        Parameter::new_deku(TransactionField::UserAccess, val)
    }
}

#[derive(Debug, Clone, Copy, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct ChatOptions(i32);

impl ChatOptions {
    pub fn none() -> Self {
        Self(0)
    }
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self::none()
    }
}

impl TryFrom<&Parameter> for ChatOptions {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.read_deku()
            .map_err(|_| ProtocolError::MalformedData(TransactionField::ChatOptions))
    }
}

impl From<ChatOptions> for Parameter {
    fn from(val: ChatOptions) -> Self {
        Parameter::new_deku(TransactionField::ChatOptions, val)
    }
}

#[derive(Debug, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord, DekuRead, DekuWrite)]
#[deku(endian = "big")]
#[into(i16, i32)]
pub struct ChatId(i16);

impl Default for ChatId {
    fn default() -> Self {
        1.into()
    }
}

impl TryFrom<&Parameter> for ChatId {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.read_deku()
            .map_err(|_| ProtocolError::MalformedData(TransactionField::ChatId))
    }
}

impl From<ChatId> for Parameter {
    fn from(val: ChatId) -> Self {
        Parameter::new_deku(TransactionField::ChatId, val)
    }
}

#[derive(Debug, Clone, From, Into, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChatSubject(Vec<u8>);

impl TryFrom<&Parameter> for ChatSubject {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let subject = take_if_matches(
            parameter.clone(),
            TransactionField::ChatSubject,
        )?;
        Ok(subject.into())
    }
}

impl From<ChatSubject> for Parameter {
    fn from(val: ChatSubject) -> Self {
        Parameter::new(TransactionField::ChatSubject, val.0)
    }
}

#[derive(Debug, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct IconId(i16);

impl From<IconId> for Parameter {
    fn from(val: IconId) -> Self {
        Parameter::new_deku(TransactionField::UserIconId, val)
    }
}

impl TryFrom<&Parameter> for IconId {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.read_deku()
            .map_err(|_| ProtocolError::MalformedData(TransactionField::UserIconId))
    }
}

#[derive(Debug, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord, Hash, Default, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct UserId(i16);

impl TryFrom<&Parameter> for UserId {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.read_deku()
            .map_err(|_| ProtocolError::MalformedData(TransactionField::UserId))
    }
}

impl From<UserId> for Parameter {
    fn from(val: UserId) -> Self {
        Parameter::new_deku(TransactionField::UserId, val)
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct UserFlags(i16);

impl From<UserFlags> for Parameter {
    fn from(val: UserFlags) -> Self {
        Parameter::new_deku(TransactionField::UserFlags, val)
    }
}

impl TryFrom<&Parameter> for UserFlags {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.read_deku()
            .map_err(|_| ProtocolError::MalformedData(TransactionField::UserFlags))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, DekuRead, DekuWrite)]
pub struct UserNameWithInfo {
    pub user_id: UserId,
    pub icon_id: IconId,
    pub user_flags: UserFlags,
    #[deku(endian = "big", update = "self.username.len() as i16")]
    pub username_len: i16,
    #[deku(ctx = "*username_len as usize")]
    pub username: Nickname,
}

impl UserNameWithInfo {
    pub fn anonymous(username: Nickname, icon_id: IconId) -> Self {
        Self {
            username_len: username.len() as i16,
            username,
            icon_id,
            user_flags: Default::default(),
            user_id: Default::default(),
        }
    }
}

impl TryFrom<&Parameter> for UserNameWithInfo {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.read_deku()
            .map_err(|_| ProtocolError::MalformedData(TransactionField::UserNameWithInfo))
    }
}

impl From<UserNameWithInfo> for Parameter {
    fn from(val: UserNameWithInfo) -> Self {
        Parameter::new_deku(TransactionField::UserNameWithInfo, val)
    }
}

#[derive(Debug, Clone, From, Into, PartialEq, Eq, PartialOrd, Ord)]
pub struct Message(Vec<u8>);

impl Message {
    pub fn new(message: Vec<u8>) -> Self {
        Self(message)
    }
}

impl From<&Parameter> for Message {
    fn from(parameter: &Parameter) -> Self {
        Self(parameter.clone().take())
    }
}

impl From<Message> for Parameter {
    fn from(val: Message) -> Self {
        Parameter::new_data(val.0)
    }
}

#[derive(Clone, From, Into, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileName(Vec<u8>);

impl From<&Parameter> for FileName {
    fn from(parameter: &Parameter) -> Self {
        Self(parameter.clone().take())
    }
}

impl From<FileName> for Parameter {
    fn from(val: FileName) -> Self {
        Parameter::new(TransactionField::FileName, val.0)
    }
}

impl Debug for FileName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = MACINTOSH.decode(&self.0);
        f.debug_tuple("FileName")
            .field(&text)
            .finish()
    }
}

impl From<&FileName> for PathBuf {
    fn from(value: &FileName) -> Self {
        let (s, _, _) = MACINTOSH.decode(&value.0);
        s.to_string().into()
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct FileSize(i32);

impl TryFrom<&Parameter> for FileSize {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.read_deku()
            .map_err(|_| ProtocolError::MalformedData(TransactionField::FileSize))
    }
}

impl From<FileSize> for Parameter {
    fn from(val: FileSize) -> Self {
        Self::new_deku(TransactionField::FileSize, val)
    }
}

#[derive(Clone)]
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
    fn encode_path_component(component: Vec<u8>) -> Vec<u8> {
        let component_length = component.len() as i8;
        [
            &[0u8; 2][..],
            &component_length.to_be_bytes(),
            component.as_slice(),
        ].iter()
            .flat_map(|b| b.iter())
            .copied()
            .collect()
    }
    fn encode_parameter(components: Vec<Vec<u8>>) -> Parameter {
        let depth = components.len() as i16;
        let components = components.into_iter()
            .map(Self::encode_path_component);
        let data = std::iter::once(depth.to_be_bytes().to_vec())
            .chain(components)
            .flat_map(|b| b.into_iter())
            .collect();
        Parameter::new(
            TransactionField::FilePath,
            data,
        )
    }
}

impl Default for FilePath {
    fn default() -> Self {
        Self::Root
    }
}

impl fmt::Debug for FilePath {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root => write!(f, "{:?}", "::"),
            Self::Directory(parts) => {
                let pathname: String = parts.iter()
                    .map(|part| MACINTOSH.decode(part))
                    .map(|enc| enc.0)
                    .collect::<Vec<_>>()
                    .join(":");
                write!(f, "{:?}", pathname)
            },
        }
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

impl TryFrom<Option<&Parameter>> for FilePath {
    type Error = ProtocolError;
    fn try_from(parameter: Option<&Parameter>) -> Result<Self, Self::Error> {
        if let Some(parameter) = parameter {
            parameter.try_into()
        } else {
            Ok(Self::Root)
        }
    }
}

impl From<FilePath> for Option<Parameter> {
    fn from(val: FilePath) -> Self {
        if let FilePath::Directory(path) = val {
            Some(FilePath::encode_parameter(path))
        } else {
            None
        }
    }
}

#[derive(Clone, From, Into)]
pub struct FileComment(Vec<u8>);

impl TryFrom<&Parameter> for FileComment {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let data = take_if_matches(parameter.clone(), TransactionField::FileComment)?;
        Ok(data.into())
    }
}

impl From<FileComment> for Parameter {
    fn from(val: FileComment) -> Self {
        Parameter::new(TransactionField::FileComment, val.0)
    }
}

impl std::fmt::Debug for FileComment {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let (comment, _, _) = MACINTOSH.decode(&self.0);
        f.debug_tuple("FileComment").field(&comment).finish()
    }
}

#[derive(Debug, Clone, Copy, From, Into, DekuRead, DekuWrite)]
pub struct FileType(pub [u8; 4]);

impl TryFrom<&Parameter> for FileType {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let data = take_if_matches(parameter.clone(), TransactionField::FileType)?;
        if data.len() != 4 {
            Err(ProtocolError::MalformedData(TransactionField::FileType))?;
        }
        let mut file_type = [0u8; 4];
        file_type.copy_from_slice(&data[..4]);
        Ok(file_type.into())
    }
}

impl From<FileType> for Parameter {
    fn from(val: FileType) -> Self {
        Parameter::new(TransactionField::FileType, val.0.to_vec())
    }
}

#[derive(Debug, Clone, Copy, From, Into, DekuRead, DekuWrite)]
pub struct Creator(pub [u8; 4]);

#[derive(Debug, Clone, From, Into)]
pub struct FileTypeString(Vec<u8>);

impl From<&FileType> for FileTypeString {
    fn from(type_code: &FileType) -> Self {
        Self(type_code.0.to_vec())
    }
}

impl TryFrom<&Parameter> for FileTypeString {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let data = take_if_matches(
            parameter.clone(),
            TransactionField::FileTypeString,
        )?;
        Ok(data.into())
    }
}

impl From<FileTypeString> for Parameter {
    fn from(val: FileTypeString) -> Self {
        Parameter::new(TransactionField::FileTypeString, val.0.to_vec())
    }
}

#[derive(Debug, Clone, From, Into)]
pub struct FileCreatorString(Vec<u8>);

impl TryFrom<&Parameter> for FileCreatorString {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let data = take_if_matches(
            parameter.clone(),
            TransactionField::FileCreatorString,
        )?;
        Ok(data.into())
    }
}

impl From<FileCreatorString> for Parameter {
    fn from(val: FileCreatorString) -> Self {
        Parameter::new(TransactionField::FileCreatorString, val.0.to_vec())
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, PartialEq, Eq, DekuRead, DekuWrite)]
pub struct FileCreatedAt(DateParameter);

impl From<SystemTime> for FileCreatedAt {
    fn from(time: SystemTime) -> Self {
        Self(time.into())
    }
}

impl TryFrom<&Parameter> for FileCreatedAt {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let data = take_if_matches(
            parameter.clone(),
            TransactionField::FileCreateDate,
        )?;
        Self::try_from(&data[..])
            .map_err(|_| ProtocolError::MalformedData(TransactionField::FileCreateDate))
    }
}

impl From<FileCreatedAt> for Parameter {
    fn from(val: FileCreatedAt) -> Self {
        Parameter::new_deku(TransactionField::FileCreateDate, val)
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, PartialEq, Eq, DekuRead, DekuWrite)]
pub struct FileModifiedAt(DateParameter);

impl From<SystemTime> for FileModifiedAt {
    fn from(time: SystemTime) -> Self {
        Self(time.into())
    }
}

impl TryFrom<&Parameter> for FileModifiedAt {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let data = take_if_matches(
            parameter.clone(),
            TransactionField::FileModifyDate,
        )?;
        Self::try_from(&data[..])
            .map_err(|_| ProtocolError::MalformedData(TransactionField::FileModifyDate))
    }
}

impl From<FileModifiedAt> for Parameter {
    fn from(val: FileModifiedAt) -> Self {
        Parameter::new_deku(TransactionField::FileModifyDate, val)
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct TransferSize(i32);

impl TryFrom<&Parameter> for TransferSize {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.read_deku()
            .map_err(|_| ProtocolError::MalformedData(TransactionField::TransferSize))
    }
}

impl From<TransferSize> for Parameter {
    fn from(val: TransferSize) -> Self {
        Self::new_deku(TransactionField::TransferSize, val)
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct ReferenceNumber(i32);

impl TryFrom<&Parameter> for ReferenceNumber {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.read_deku()
            .map_err(|_| ProtocolError::MalformedData(TransactionField::ReferenceNumber))
    }
}

impl From<ReferenceNumber> for Parameter {
    fn from(val: ReferenceNumber) -> Self {
        Self::new_deku(TransactionField::ReferenceNumber, val)
    }
}

impl HotlineProtocol for ReferenceNumber {
    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().unwrap()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let ((bytes, _bits), value) = <Self as DekuContainerRead>::from_bytes((bytes, 0)).unwrap();
        Ok((bytes, value))
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct WaitingCount(i32);

impl TryFrom<&Parameter> for WaitingCount {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.read_deku()
            .map_err(|_| ProtocolError::MalformedData(TransactionField::WaitingCount))
    }
}

impl From<WaitingCount> for Parameter {
    fn from(val: WaitingCount) -> Self {
        Self::new_deku(TransactionField::WaitingCount, val)
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct TransactionOptions(i32);

impl TryFrom<&Parameter> for TransactionOptions {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.read_deku()
         .map_err(|_| ProtocolError::MalformedData(TransactionField::Options))
    }
}

impl From<TransactionOptions> for Parameter {
    fn from(val: TransactionOptions) -> Self {
        Parameter::new_deku(TransactionField::Options, val)
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
