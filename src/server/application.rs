use std::{future::Future, fmt, marker::PhantomData, pin::Pin};
use derive_more::{From, Into};
use enumset::{EnumSetType, EnumSet, EnumSetIter, enum_set};
use serde::{de::Visitor, ser::SerializeMap, Serialize, Deserialize, Serializer, Deserializer};
use strum::{EnumIter, Display, IntoEnumIterator, EnumString};

type PBDF<O> = Pin<Box<dyn Future<Output=O>>>;
type PBDFR<O> = PBDF<Result<O, Error>>;

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

#[derive(Debug, Serialize, Deserialize, Display, PartialOrd, Ord, EnumIter, EnumString, EnumSetType)]
#[strum(serialize_all = "snake_case")]
#[serde(try_from = "&str")]
pub enum FileOperation {
    Download,
    UploadToDropbox,
    UploadToFolder,
    DeleteFile,
    RenameFile,
    MoveFile,
    SetFileComment,
    CreateFolder,
    DeleteFolder,
    RenameFolder,
    MoveFolder,
    SetFolderComment,
    ViewDropBox,
    CreateAlias,
}

#[derive(Debug, Serialize, Deserialize, Display, PartialOrd, Ord, EnumIter, EnumString, EnumSetType)]
#[strum(serialize_all = "snake_case")]
#[serde(try_from = "&str")]
pub enum UserOperation {
    CanCreateUsers,
    CanDeleteUsers,
    CanReadUsers,
    CanModifyUsers,
    CanGetUserInfo,
    CanDisconnectUsers,
    CannotBeDisconnected,
}

#[derive(Debug, Serialize, Deserialize, Display, PartialOrd, Ord, EnumIter, EnumString, EnumSetType)]
#[strum(serialize_all = "snake_case")]
#[serde(try_from = "&str")]
pub enum NewsOperation {
    ReadNews,
    PostNews,
}

#[derive(Debug, Serialize, Deserialize, Display, PartialOrd, Ord, EnumIter, EnumString, EnumSetType)]
#[strum(serialize_all = "snake_case")]
#[serde(try_from = "&str")]
pub enum ChatOperation {
    ReadChat,
    SendChat,
}

#[derive(Debug, Serialize, Deserialize, Display, PartialOrd, Ord, EnumIter, EnumString, EnumSetType)]
#[strum(serialize_all = "snake_case")]
#[serde(try_from = "&str")]
pub enum MiscOperation {
    CanUseAnyName,
    DontShowAgreement,
}

#[derive(Debug, Clone, From, Into, PartialOrd, Ord, PartialEq, Eq)]
struct FlagSet<T: EnumSetType + IntoEnumIterator + fmt::Display>(EnumSet<T>);

impl <F> FromIterator<F> for FlagSet<F>
    where F: EnumSetType + IntoEnumIterator + fmt::Display {
    fn from_iter<T: IntoIterator<Item = F>>(iter: T) -> Self {
        iter.into_iter().collect::<EnumSet<F>>().into()
    }
}

impl <T> Serialize for FlagSet<T>
    where T: EnumSetType + IntoEnumIterator + fmt::Display {
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

impl <T> FlagSetVisitor<T> {
    fn new() -> Self { Self(PhantomData) }
}

impl <'de, E> Visitor<'de> for FlagSetVisitor<FlagSet<E>>
    where E: EnumSetType + IntoEnumIterator + Deserialize<'de> + fmt::Display {
    type Value = FlagSet<E>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "flag set")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where A: serde::de::MapAccess<'de> {
        let mut flags = EnumSet::empty();
        while let Some((key, value)) = map.next_entry::<E, bool>()? {
            if value {
                flags.insert(key);
            }
        }
        Ok(flags.into())
    }
}

impl <'de, T> Deserialize<'de> for FlagSet<T>
    where T: EnumSetType + IntoEnumIterator + Deserialize<'de> + fmt::Display {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(FlagSetVisitor::new())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, From)]
#[serde(transparent)]
pub struct FilePermissions(FlagSet<FileOperation>);

impl Permissions<FileOperation> for FilePermissions {
    fn can(&self, op: FileOperation) -> bool {
        self.0.0.contains(op)
    }
}

impl IntoIterator for FilePermissions {
    type Item = FileOperation;
    type IntoIter = EnumSetIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.0.into_iter()
    }
}

impl FromIterator<FileOperation> for FilePermissions {
    fn from_iter<T: IntoIterator<Item = FileOperation>>(iter: T) -> Self {
        let flags: FlagSet<_> = iter.into_iter().collect();
        flags.into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, From)]
#[serde(transparent)]
pub struct UserPermissions(FlagSet<UserOperation>);

impl Permissions<UserOperation> for UserPermissions {
    fn can(&self, op: UserOperation) -> bool {
        self.0.0.contains(op)
    }
}

impl IntoIterator for UserPermissions {
    type Item = UserOperation;
    type IntoIter = EnumSetIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.0.into_iter()
    }
}

impl FromIterator<UserOperation> for UserPermissions {
    fn from_iter<T: IntoIterator<Item = UserOperation>>(iter: T) -> Self {
        let flags: FlagSet<_> = iter.into_iter().collect();
        flags.into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, From)]
#[serde(transparent)]
pub struct NewsPermissions(FlagSet<NewsOperation>);

impl Permissions<NewsOperation> for NewsPermissions {
    fn can(&self, op: NewsOperation) -> bool {
        self.0.0.contains(op)
    }
}

impl IntoIterator for NewsPermissions {
    type Item = NewsOperation;
    type IntoIter = EnumSetIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.0.into_iter()
    }
}

impl FromIterator<NewsOperation> for NewsPermissions {
    fn from_iter<T: IntoIterator<Item = NewsOperation>>(iter: T) -> Self {
        let flags: FlagSet<_> = iter.into_iter().collect();
        flags.into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, From)]
#[serde(transparent)]
pub struct ChatPermissions(FlagSet<ChatOperation>);

impl Permissions<ChatOperation> for ChatPermissions {
    fn can(&self, op: ChatOperation) -> bool {
        self.0.0.contains(op)
    }
}

impl IntoIterator for ChatPermissions {
    type Item = ChatOperation;
    type IntoIter = EnumSetIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.0.into_iter()
    }
}

impl FromIterator<ChatOperation> for ChatPermissions {
    fn from_iter<T: IntoIterator<Item = ChatOperation>>(iter: T) -> Self {
        let flags: FlagSet<_> = iter.into_iter().collect();
        flags.into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, From)]
#[serde(transparent)]
pub struct MiscPermissions(FlagSet<MiscOperation>);
impl Permissions<MiscOperation> for MiscPermissions {
    fn can(&self, op: MiscOperation) -> bool {
        self.0.0.contains(op)
    }
}

impl IntoIterator for MiscPermissions {
    type Item = MiscOperation;
    type IntoIter = EnumSetIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.0.into_iter()
    }
}

impl FromIterator<MiscOperation> for MiscPermissions {
    fn from_iter<T: IntoIterator<Item = MiscOperation>>(iter: T) -> Self {
        let flags: FlagSet<_> = iter.into_iter().collect();
        flags.into()
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default, From, Into)]
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
    fn online(&self) -> PBDFR<UserList>;
    fn info(&self, user: &OnlineUser) -> PBDFR<UserInfo>;
    fn authenticate(&self, credentials: &Credentials) -> PBDFR<bool>;
    fn authorize<I: Identity>(&self, identity: &I) -> PBDFR<bool>;
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

impl <U: Users, F: Files, N: News, M: Messages> Application<U, F, N, M> {

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
        fn online(&self) -> PBDFR<UserList> {
            let users = UserList { users: vec![test_user()] };
            pbfutrok(users)
        }

        fn info(&self, _: &OnlineUser) -> PBDFR<UserInfo> {
            let user = test_user();
            pbfutrok(user)
        }

        fn authenticate(&self, _: &Credentials) -> PBDFR<bool> {
            pbfutrok(true)
        }

        fn authorize<I: Identity>(&self, _: &I) -> PBDFR<bool> {
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
        assert_eq!(who.users, vec![ test_user() ]);
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
                password: "password".into(),
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
