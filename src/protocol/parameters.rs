use super::{
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

use std::time::SystemTime;

use derive_more::{From, Into};

pub trait Credential {
    fn deobfuscate(&self) -> Vec<u8>;
}

fn invert_credential(data: &[u8]) -> Vec<u8> {
    data.iter()
        .map(|b| !b)
        .collect()
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, From, Into)]
pub struct Nickname(Vec<u8>);

impl Nickname {
    pub fn new(nickname: Vec<u8>) -> Self {
        Self(nickname)
    }
    pub fn take(self) -> Vec<u8> {
        self.0
    }
}

impl std::fmt::Debug for Nickname {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = String::from_utf8_lossy(self.0.as_slice());
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

impl Into<Parameter> for Nickname {
    fn into(self) -> Parameter {
        let Self(field_data) = self;
        Parameter::new(TransactionField::UserName.into(), field_data)
    }
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
        Parameter::new(TransactionField::UserLogin.into(), field_data)
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

impl Into<Parameter> for Password {
    fn into(self) -> Parameter {
        let Self(password) = self;
        Parameter::new(
            TransactionField::UserPassword.into(),
            password,
        )
    }
}

impl Credential for Password {
    fn deobfuscate(&self) -> Vec<u8> {
        invert_credential(&self.0)
    }
}

#[derive(Debug, Clone, Copy, From, Into)]
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
        parameter.clone()
         .int()
         .map(|i| Self(i.into()))
         .ok_or(ProtocolError::MalformedData(TransactionField::ChatOptions))
    }
}

impl Into<Parameter> for ChatOptions {
    fn into(self) -> Parameter {
        let Self(int) = self;
        Parameter::new_int(
            TransactionField::ChatOptions.into(),
            int.into(),
        )
    }
}

#[derive(Debug, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChatId(i32);

impl Default for ChatId {
    fn default() -> Self {
        1.into()
    }
}

impl TryFrom<&Parameter> for ChatId {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.clone()
         .int()
         .map(|i| Self(i.into()))
         .ok_or(ProtocolError::MalformedData(TransactionField::ChatId))
    }
}

impl Into<Parameter> for ChatId {
    fn into(self) -> Parameter {
        let Self(int) = self;
        Parameter::new_int(
            TransactionField::ChatId.into(),
            int.into(),
        )
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

impl Into<Parameter> for ChatSubject {
    fn into(self) -> Parameter {
        let Self(subject) = self;
        Parameter::new(
            TransactionField::ChatSubject.into(),
            subject,
        )
    }
}

#[derive(Debug, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord)]
pub struct IconId(i16);

impl Into<Parameter> for IconId {
    fn into(self) -> Parameter {
        let Self(int) = self;
        Parameter::new_int(
            TransactionField::UserIconId.into(),
            int.into(),
        )
    }
}

impl TryFrom<&Parameter> for IconId {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.int()
            .and_then(|int| int.i16())
            .map(Self::from)
            .ok_or(ProtocolError::MalformedData(TransactionField::UserIconId))
    }
}

#[derive(
    Debug, Clone, Copy,
    From, Into,
    PartialEq, Eq,
    PartialOrd, Ord,
    Hash,
)]
pub struct UserId(i16);

impl TryFrom<&Parameter> for UserId {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.int()
            .and_then(|i| i.i16())
            .map(Self::from)
            .ok_or(ProtocolError::MalformedData(TransactionField::UserId))
    }
}

impl Into<Parameter> for UserId {
    fn into(self) -> Parameter {
        let Self(int) = self;
        Parameter::new_int(
            TransactionField::UserId.into(),
            int.into(),
        )
    }
}

#[derive(Debug, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord)]
pub struct UserFlags(i16);

impl Into<Parameter> for UserFlags {
    fn into(self) -> Parameter {
        let Self(int) = self;
        Parameter::new_int(
            TransactionField::UserFlags.into(),
            int.into(),
        )
    }
}

impl TryFrom<&Parameter> for UserFlags {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.int()
            .and_then(|i| i.i16())
            .map(Self::from)
            .ok_or(ProtocolError::MalformedData(TransactionField::UserFlags))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserNameWithInfo {
    pub user_id: UserId,
    pub icon_id: IconId,
    pub user_flags: UserFlags,
    pub username: Nickname,
}

impl UserNameWithInfo {
    fn user_id(bytes: &[u8]) -> BIResult<UserId> {
        let (bytes, id) = be_i16(bytes)?;
        Ok((bytes, id.into()))
    }
    fn icon_id(bytes: &[u8]) -> BIResult<IconId> {
        let (bytes, id) = be_i16(bytes)?;
        Ok((bytes, id.into()))
    }
    fn user_flags(bytes: &[u8]) -> BIResult<UserFlags> {
        let (bytes, flags) = be_i16(bytes)?;
        Ok((bytes, flags.into()))
    }
    fn username(bytes: &[u8]) -> BIResult<Nickname> {
        let (bytes, len) = be_i16(bytes)?;
        let (bytes, username) = take(len as usize)(bytes)?;
        Ok((bytes, username.to_vec().into()))
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, user_id) = Self::user_id(bytes)?;
        let (bytes, icon_id) = Self::icon_id(bytes)?;
        let (bytes, user_flags) = Self::user_flags(bytes)?;
        let (bytes, username) = Self::username(bytes)?;
        Ok((
            bytes,
            Self {
                user_id,
                icon_id,
                user_flags,
                username,
            },
        ))
    }
}

impl TryFrom<&Parameter> for UserNameWithInfo {
    type Error = ProtocolError;
    fn try_from(p: &Parameter) -> Result<Self, Self::Error> {
        let bytes = p.borrow();
        Self::from_bytes(bytes)
            .map(|(_, user)| user)
            .map_err(|_| ProtocolError::MalformedData(TransactionField::UserNameWithInfo))
    }
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

impl Into<Parameter> for Message {
    fn into(self) -> Parameter {
        Parameter::new(
            TransactionField::Data.into(),
            self.0,
        )
    }
}

#[derive(Debug, Clone, From, Into, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileName(Vec<u8>);

impl From<&Parameter> for FileName {
    fn from(parameter: &Parameter) -> Self {
        Self(parameter.clone().take())
    }
}

impl Into<Parameter> for FileName {
    fn into(self) -> Parameter {
        Parameter::new(
            TransactionField::FileName.into(),
            self.0,
        )
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into)]
pub struct FileSize(i32);

impl TryFrom<&Parameter> for FileSize {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        parameter.int()
            .ok_or(ProtocolError::MalformedData(TransactionField::FileSize))
            .map(|p| Self(p.into()))
    }
}

impl Into<Parameter> for FileSize {
    fn into(self) -> Parameter {
        Parameter::new_int(
            TransactionField::FileSize.into(),
            self.0.into(),
        )
    }
}

#[derive(Debug, Clone)]
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
        vec![
            &[0u8; 2][..],
            &component_length.to_be_bytes()[..],
            &component.as_slice()[..],
        ].into_iter()
            .flat_map(|b| b.into_iter())
            .map(|b| *b)
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
            TransactionField::FilePath.into(),
            data,
        )
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

impl Into<Option<Parameter>> for FilePath {
    fn into(self) -> Option<Parameter> {
        if let Self::Directory(path) = self {
            Some(Self::encode_parameter(path))
        } else {
            None
        }
    }
}

#[derive(Debug, From, Into)]
pub struct FileComment(Vec<u8>);

impl TryFrom<&Parameter> for FileComment {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let data = take_if_matches(parameter.clone(), TransactionField::FileComment)?;
        Ok(data.into())
    }
}

impl Into<Parameter> for FileComment {
    fn into(self) -> Parameter {
        Parameter::new(
            TransactionField::FileComment.into(),
            self.0
        )
    }
}

#[derive(Debug, Clone, Copy, From, Into)]
pub struct FileType(pub [u8; 4]);

impl From<&[u8; 4]> for FileType {
    fn from(data: &[u8; 4]) -> Self {
        data.to_owned().into()
    }
}

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

impl Into<Parameter> for FileType {
    fn into(self) -> Parameter {
        Parameter::new(
            TransactionField::FileType.into(),
            self.0.to_vec()
        )
    }
}

#[derive(Debug, Clone, Copy, From, Into)]
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

impl Into<Parameter> for FileTypeString {
    fn into(self) -> Parameter {
        Parameter::new(
            TransactionField::FileTypeString.into(),
            self.0.to_vec()
        )
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

impl Into<Parameter> for FileCreatorString {
    fn into(self) -> Parameter {
        Parameter::new(
            TransactionField::FileCreatorString.into(),
            self.0.to_vec()
        )
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, PartialEq, Eq)]
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
        let (tail, date) = DateParameter::parse(&data)
            .map_err(|_| ProtocolError::MalformedData(TransactionField::FileCreateDate))?;
        if !tail.is_empty() {
            Err(ProtocolError::MalformedData(TransactionField::FileCreateDate))?;
        }
        Ok(Self(date))
    }
}

impl Into<Parameter> for FileCreatedAt {
    fn into(self) -> Parameter {
        let Self(date) = self;
        Parameter::new(
            TransactionField::FileCreateDate.into(),
            date.pack(),
        )
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, PartialEq, Eq)]
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
        let (tail, date) = DateParameter::parse(&data)
            .map_err(|_| ProtocolError::MalformedData(TransactionField::FileModifyDate))?;
        if !tail.is_empty() {
            Err(ProtocolError::MalformedData(TransactionField::FileModifyDate))?;
        }
        Ok(Self(date))
    }
}

impl Into<Parameter> for FileModifiedAt {
    fn into(self) -> Parameter {
        let Self(date) = self;
        Parameter::new(
            TransactionField::FileModifyDate.into(),
            date.pack(),
        )
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, PartialEq, Eq)]
pub struct TransferSize(i32);

impl TryFrom<&Parameter> for TransferSize {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let size = parameter.int()
            .map(i32::from)
            .ok_or(ProtocolError::MalformedData(TransactionField::TransferSize))?
            .into();
        Ok(Self(size))
    }
}

impl Into<Parameter> for TransferSize {
    fn into(self) -> Parameter {
        let Self(size) = self;
        Parameter::new_int(
            TransactionField::TransferSize,
            size,
        )
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, PartialEq, Eq)]
pub struct ReferenceNumber(i32);

impl TryFrom<&Parameter> for ReferenceNumber {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let reference = parameter.int()
            .map(i32::from)
            .ok_or(ProtocolError::MalformedData(TransactionField::ReferenceNumber))?
            .into();
        Ok(Self(reference))
    }
}

impl Into<Parameter> for ReferenceNumber {
    fn into(self) -> Parameter {
        let Self(reference) = self;
        Parameter::new_int(
            TransactionField::ReferenceNumber,
            reference,
        )
    }
}

#[derive(Debug, Default, Clone, Copy, From, Into, PartialEq, Eq)]
pub struct WaitingCount(i32);

impl TryFrom<&Parameter> for WaitingCount {
    type Error = ProtocolError;
    fn try_from(parameter: &Parameter) -> Result<Self, Self::Error> {
        let reference = parameter.int()
            .map(i32::from)
            .ok_or(ProtocolError::MalformedData(TransactionField::WaitingCount))?
            .into();
        Ok(Self(reference))
    }
}

impl Into<Parameter> for WaitingCount {
    fn into(self) -> Parameter {
        let Self(count) = self;
        Parameter::new_int(
            TransactionField::WaitingCount,
            count,
        )
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
