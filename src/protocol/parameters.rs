use super::{
    ProtocolError,
    transaction::{Parameter, IntParameter},
    transaction_field::TransactionField,
};

use derive_more::{From, Into};

pub trait Credential {
    fn deobfuscate(&self) -> Vec<u8>;
}

fn invert_credential(data: &[u8]) -> Vec<u8> {
    data.iter()
        .map(|b| !b)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, From, Into)]
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

#[derive(Debug, Clone, Copy, From, Into)]
pub struct ChatId(i32);

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

#[derive(Debug, Clone, Copy, From, Into, PartialEq, Eq, PartialOrd, Ord)]
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
