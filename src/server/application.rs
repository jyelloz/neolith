use derive_more::{From, Into};
use enumset::{enum_set, EnumSet, EnumSetIter, EnumSetType};
use serde::{de::Visitor, ser::SerializeMap, Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, future::Future, marker::PhantomData, pin::Pin};
use strum::{Display, EnumIter, EnumString, IntoEnumIterator};

use crate::protocol as proto;

type Pbdf<O> = Pin<Box<dyn Future<Output = O>>>;
type Ppdfr<O> = Pbdf<Result<O, Error>>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Authentication Failure")]
    AuthenticationFailure,
    #[error("Authorization Failure")]
    AuthorizationFailure,
}

pub trait Identity {}
pub trait Permissions<O> {
    fn can(&self, op: O) -> bool;
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Credentials {
    username: String,
    password: String,
}

#[derive(
    Debug, Serialize, Deserialize, Display, PartialOrd, Ord, EnumIter, EnumString, EnumSetType,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum FileOperation {
    Download = 2,
    UploadToDropbox = 1,
    UploadToFolder = 25,
    DeleteFile = 0,
    RenameFile = 3,
    MoveFile = 4,
    SetFileComment = 28,
    CreateFolder = 5,
    DeleteFolder = 6,
    RenameFolder = 7,
    MoveFolder = 8,
    SetFolderComment = 29,
    ViewDropBox = 30,
    CreateAlias = 31,
}

#[derive(
    Debug, Serialize, Deserialize, Display, PartialOrd, Ord, EnumIter, EnumString, EnumSetType,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum UserOperation {
    CanCreateUsers = 14,
    CanDeleteUsers = 15,
    CanReadUsers = 16,
    CanModifyUsers = 17,
    CanGetUserInfo = 24,
    CanDisconnectUsers = 22,
    CannotBeDisconnected = 23,
}

#[derive(
    Debug, Serialize, Deserialize, Display, PartialOrd, Ord, EnumIter, EnumString, EnumSetType,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum NewsOperation {
    ReadNews = 20,
    PostNews = 21,
}

#[derive(
    Debug, Serialize, Deserialize, Display, PartialOrd, Ord, EnumIter, EnumString, EnumSetType,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ChatOperation {
    ReadChat = 9,
    SendChat = 10,
}

#[derive(
    Debug, Serialize, Deserialize, Display, PartialOrd, Ord, EnumIter, EnumString, EnumSetType,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum MiscOperation {
    CanUseAnyName = 26,
    DontShowAgreement = 27,
}

#[derive(Debug, Clone, From, Into, PartialOrd, Ord, PartialEq, Eq)]
struct FlagSet<T: EnumSetType + IntoEnumIterator + fmt::Display>(EnumSet<T>);

impl<T: EnumSetType + IntoEnumIterator + fmt::Display> FlagSet<T> {
    pub fn empty() -> Self {
        Self(EnumSet::new())
    }
}

impl<F> FromIterator<F> for FlagSet<F>
where
    F: EnumSetType + IntoEnumIterator + fmt::Display,
{
    fn from_iter<T: IntoIterator<Item = F>>(iter: T) -> Self {
        iter.into_iter().collect::<EnumSet<F>>().into()
    }
}

impl<T> Serialize for FlagSet<T>
where
    T: EnumSetType + IntoEnumIterator + fmt::Display,
{
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let Self(flags) = self;
        let mut map = ser.serialize_map(None)?;
        for flag in T::iter() {
            map.serialize_entry(&flag.to_string(), &flags.contains(flag))?;
        }
        map.end()
    }
}

struct FlagSetVisitor<T>(PhantomData<T>);

impl<T> FlagSetVisitor<T> {
    fn new() -> Self {
        Self(PhantomData)
    }
}

impl<'de, E> Visitor<'de> for FlagSetVisitor<FlagSet<E>>
where
    E: EnumSetType + IntoEnumIterator + Deserialize<'de> + fmt::Display,
{
    type Value = FlagSet<E>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "flag set")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut flags = EnumSet::empty();
        while let Some((key, value)) = map.next_entry::<E, bool>()? {
            if value {
                flags.insert(key);
            }
        }
        Ok(flags.into())
    }
}

impl<'de, T> Deserialize<'de> for FlagSet<T>
where
    T: EnumSetType + IntoEnumIterator + Deserialize<'de> + fmt::Display,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(FlagSetVisitor::new())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, From)]
#[serde(transparent)]
pub struct FilePermissions(FlagSet<FileOperation>);

impl Permissions<FileOperation> for FilePermissions {
    fn can(&self, op: FileOperation) -> bool {
        self.0 .0.contains(op)
    }
}

impl IntoIterator for FilePermissions {
    type Item = FileOperation;
    type IntoIter = EnumSetIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0 .0.into_iter()
    }
}

impl FromIterator<FileOperation> for FilePermissions {
    fn from_iter<T: IntoIterator<Item = FileOperation>>(iter: T) -> Self {
        let flags: FlagSet<_> = iter.into_iter().collect();
        flags.into()
    }
}

impl From<i64> for FilePermissions {
    fn from(bits: i64) -> Self {
        let mut flags = FlagSet::empty();
        for op in FileOperation::iter() {
            let bit = 1 & (bits >> (63 - (op as u8))) == 1;
            if bit {
                flags.0.insert(op);
            }
        }
        Self(flags)
    }
}

impl From<FilePermissions> for i64 {
    fn from(val: FilePermissions) -> Self {
        let mut bits = 0;
        for op in val.into_iter() {
            bits |= 1 << (63 - (op as u8));
        }
        bits
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, From)]
#[serde(transparent)]
pub struct UserPermissions(FlagSet<UserOperation>);

impl Permissions<UserOperation> for UserPermissions {
    fn can(&self, op: UserOperation) -> bool {
        self.0 .0.contains(op)
    }
}

impl IntoIterator for UserPermissions {
    type Item = UserOperation;
    type IntoIter = EnumSetIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0 .0.into_iter()
    }
}

impl FromIterator<UserOperation> for UserPermissions {
    fn from_iter<T: IntoIterator<Item = UserOperation>>(iter: T) -> Self {
        let flags: FlagSet<_> = iter.into_iter().collect();
        flags.into()
    }
}

impl From<i64> for UserPermissions {
    fn from(bits: i64) -> Self {
        let mut flags = FlagSet::empty();
        for op in UserOperation::iter() {
            let bit = 1 & (bits >> (63 - (op as u8))) == 1;
            if bit {
                flags.0.insert(op);
            }
        }
        Self(flags)
    }
}

impl From<UserPermissions> for i64 {
    fn from(val: UserPermissions) -> Self {
        let mut bits = 0;
        for op in val.into_iter() {
            bits |= 1 << (63 - (op as u8));
        }
        bits
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, From)]
#[serde(transparent)]
pub struct NewsPermissions(FlagSet<NewsOperation>);

impl Permissions<NewsOperation> for NewsPermissions {
    fn can(&self, op: NewsOperation) -> bool {
        self.0 .0.contains(op)
    }
}

impl IntoIterator for NewsPermissions {
    type Item = NewsOperation;
    type IntoIter = EnumSetIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0 .0.into_iter()
    }
}

impl FromIterator<NewsOperation> for NewsPermissions {
    fn from_iter<T: IntoIterator<Item = NewsOperation>>(iter: T) -> Self {
        let flags: FlagSet<_> = iter.into_iter().collect();
        flags.into()
    }
}

impl From<i64> for NewsPermissions {
    fn from(bits: i64) -> Self {
        let mut flags = FlagSet::empty();
        for op in NewsOperation::iter() {
            let bit = 1 & (bits >> (63 - (op as u8))) == 1;
            if bit {
                flags.0.insert(op);
            }
        }
        Self(flags)
    }
}

impl From<NewsPermissions> for i64 {
    fn from(val: NewsPermissions) -> Self {
        let mut bits = 0;
        for op in val.into_iter() {
            bits |= 1 << (63 - (op as u8));
        }
        bits
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, From)]
#[serde(transparent)]
pub struct ChatPermissions(FlagSet<ChatOperation>);

impl Permissions<ChatOperation> for ChatPermissions {
    fn can(&self, op: ChatOperation) -> bool {
        self.0 .0.contains(op)
    }
}

impl IntoIterator for ChatPermissions {
    type Item = ChatOperation;
    type IntoIter = EnumSetIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.0 .0.into_iter()
    }
}

impl FromIterator<ChatOperation> for ChatPermissions {
    fn from_iter<T: IntoIterator<Item = ChatOperation>>(iter: T) -> Self {
        let flags: FlagSet<_> = iter.into_iter().collect();
        flags.into()
    }
}

impl From<i64> for ChatPermissions {
    fn from(bits: i64) -> Self {
        let mut flags = FlagSet::empty();
        for op in ChatOperation::iter() {
            let bit = 1 & (bits >> (63 - (op as u8))) == 1;
            if bit {
                flags.0.insert(op);
            }
        }
        Self(flags)
    }
}

impl From<ChatPermissions> for i64 {
    fn from(val: ChatPermissions) -> Self {
        let mut bits = 0;
        for op in val.into_iter() {
            bits |= 1 << (63 - (op as u8));
        }
        bits
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, From)]
#[serde(transparent)]
pub struct MiscPermissions(FlagSet<MiscOperation>);
impl Permissions<MiscOperation> for MiscPermissions {
    fn can(&self, op: MiscOperation) -> bool {
        self.0 .0.contains(op)
    }
}

impl IntoIterator for MiscPermissions {
    type Item = MiscOperation;
    type IntoIter = EnumSetIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.0 .0.into_iter()
    }
}

impl FromIterator<MiscOperation> for MiscPermissions {
    fn from_iter<T: IntoIterator<Item = MiscOperation>>(iter: T) -> Self {
        let flags: FlagSet<_> = iter.into_iter().collect();
        flags.into()
    }
}

impl From<i64> for MiscPermissions {
    fn from(bits: i64) -> Self {
        let mut flags = FlagSet::empty();
        for op in MiscOperation::iter() {
            let bit = 1 & (bits >> (63 - (op as u8))) == 1;
            if bit {
                flags.0.insert(op);
            }
        }
        Self(flags)
    }
}

impl From<MiscPermissions> for i64 {
    fn from(val: MiscPermissions) -> Self {
        let mut bits = 0;
        for op in val.into_iter() {
            bits |= 1 << (63 - (op as u8));
        }
        bits
    }
}

impl Default for FilePermissions {
    fn default() -> Self {
        Self(enum_set!(FileOperation::Download | FileOperation::UploadToDropbox).into())
    }
}

impl Default for UserPermissions {
    fn default() -> Self {
        Self(enum_set!(UserOperation::CanGetUserInfo).into())
    }
}

impl Default for NewsPermissions {
    fn default() -> Self {
        Self(enum_set!(NewsOperation::ReadNews).into())
    }
}

impl Default for ChatPermissions {
    fn default() -> Self {
        Self(enum_set!(ChatOperation::ReadChat | ChatOperation::SendChat).into())
    }
}

impl Default for MiscPermissions {
    fn default() -> Self {
        Self(enum_set!(MiscOperation::CanUseAnyName).into())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct UserAccountPermissions {
    pub file: FilePermissions,
    pub user: UserPermissions,
    pub news: NewsPermissions,
    pub chat: ChatPermissions,
    pub misc: MiscPermissions,
}

impl From<i64> for UserAccountPermissions {
    fn from(i: i64) -> Self {
        Self {
            file: i.into(),
            user: i.into(),
            news: i.into(),
            chat: i.into(),
            misc: i.into(),
        }
    }
}

impl From<UserAccountPermissions> for i64 {
    fn from(val: UserAccountPermissions) -> Self {
        let file: i64 = val.file.into();
        let user: i64 = val.user.into();
        let news: i64 = val.news.into();
        let chat: i64 = val.chat.into();
        let misc: i64 = val.misc.into();
        file | user | news | chat | misc
    }
}

impl From<UserAccountPermissions> for proto::UserAccess {
    fn from(value: UserAccountPermissions) -> Self {
        Self::from(i64::from(value))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
#[serde(transparent)]
pub struct Password(String);

impl Password {
    pub fn verify(&self, password: &str) -> bool {
        pwhash::bcrypt::verify(password, &self.0)
    }
}
impl TryFrom<&str> for Password {
    type Error = pwhash::error::Error;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let hash = pwhash::bcrypt::hash(value)?;
        Ok(Self(hash))
    }
}

impl TryFrom<String> for Password {
    type Error = pwhash::error::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct UserAccountIdentity {
    pub name: String,
    pub login: String,
    pub password: Password,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct UserAccount {
    pub identity: UserAccountIdentity,
    pub permissions: UserAccountPermissions,
}

impl From<UserAccount> for proto::GetUserReply {
    fn from(value: UserAccount) -> Self {
        let username = proto::Nickname::from(value.identity.name);
        let user_login = proto::UserLogin::from(value.identity.login).invert();
        let user_password = proto::Password::from_cleartext(&[]);
        let user_access = proto::UserAccess::from(value.permissions);
        Self {
            username,
            user_login,
            user_password,
            user_access,
        }
    }
}

#[derive(
    Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default, From, Into,
)]
#[serde(transparent)]
pub struct UserDataFile(UserAccount);

pub struct OnlineUser;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UserInfo {
    username: String,
    nickname: String,
    icon_id: i32,
    address: String,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UserList {
    users: Vec<UserInfo>,
}

pub trait Users {
    fn online(&self) -> Ppdfr<UserList>;
    fn info(&self, user: &OnlineUser) -> Ppdfr<UserInfo>;
    fn authenticate(&self, credentials: &Credentials) -> Ppdfr<bool>;
    fn authorize<I: Identity>(&self, identity: &I) -> Ppdfr<bool>;
}
pub trait Files {}
pub trait News {}
pub trait Messages {}

impl Unpin for UserList {}

pub struct Application<U: Users, F: Files, N: News, M: Messages> {
    users: U,
    files: F,
    news: N,
    messages: M,
}

impl<U: Users, F: Files, N: News, M: Messages> Application<U, F, N, M> {
    async fn login(&self, credentials: &Credentials) -> Result<(), Error> {
        let result = self.users.authenticate(credentials).await?;
        if result {
            Ok(())
        } else {
            Err(Error::AuthenticationFailure)
        }
    }

    pub async fn who(&self) -> Result<UserList, Error> {
        let who = self.users.online();
        who.await
    }

    pub async fn info(&self, user: &OnlineUser) -> Result<UserInfo, Error> {
        let info = self.users.info(user);
        info.await
    }

    async fn command() -> Result<(), ()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::future;

    struct EmptyFiles;
    impl Files for EmptyFiles {}
    struct EmptyNews;
    impl News for EmptyNews {}
    struct EmptyMessages;
    impl Messages for EmptyMessages {}

    struct TestUsers;

    fn test_user() -> UserInfo {
        UserInfo {
            username: "username".into(),
            nickname: "nick name".into(),
            icon_id: 1234,
            address: "127.0.0.1".into(),
        }
    }

    fn futrok<T>(value: T) -> future::Ready<Result<T, Error>> {
        future::ready(Ok(value))
    }
    fn pbfutrok<T>(value: T) -> Pin<Box<future::Ready<Result<T, Error>>>> {
        Box::pin(futrok(value))
    }

    impl Users for TestUsers {
        fn online(&self) -> Ppdfr<UserList> {
            let users = UserList {
                users: vec![test_user()],
            };
            pbfutrok(users)
        }
        fn info(&self, _: &OnlineUser) -> Ppdfr<UserInfo> {
            let user = test_user();
            pbfutrok(user)
        }
        fn authenticate(&self, _: &Credentials) -> Ppdfr<bool> {
            pbfutrok(true)
        }
        fn authorize<I: Identity>(&self, _: &I) -> Ppdfr<bool> {
            pbfutrok(true)
        }
    }

    #[tokio::test]
    async fn test_who() -> Result<()> {
        let application = Application {
            users: TestUsers,
            files: EmptyFiles,
            news: EmptyNews,
            messages: EmptyMessages,
        };
        let who = application.who().await?;
        assert_eq!(who.users, vec![test_user()]);
        Ok(())
    }

    #[tokio::test]
    async fn test_info() -> Result<()> {
        let application = Application {
            users: TestUsers,
            files: EmptyFiles,
            news: EmptyNews,
            messages: EmptyMessages,
        };
        let u = OnlineUser;
        let info = application.info(&u).await?;
        assert_eq!(info, test_user());
        Ok(())
    }

    #[test]
    fn test_userdata() -> Result<()> {
        let u = UserAccount {
            identity: UserAccountIdentity {
                name: "test account".into(),
                login: "test".into(),
                password: "password".try_into()?,
            },
            ..Default::default()
        };
        let user_data = UserDataFile(u.clone());
        let s = toml::to_string(&user_data).unwrap();
        println!("{}", s);
        let UserDataFile(from) = toml::from_str::<UserDataFile>(&s)?;
        println!("{:?}", &from);
        assert_eq!(u, from);
        Ok(())
    }

    #[test]
    fn deserialize_news_permissions() -> Result<()> {
        let d = NewsPermissions::default();
        let toml = toml::to_string(&d)?;
        let perms: NewsPermissions = toml::from_str(&toml)?;
        assert_eq!(d, perms);
        Ok(())
    }
}
