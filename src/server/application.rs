use std::future::Future;
use std::pin::Pin;
use console::{
    pad_str_with,
    Alignment,
};
use derive_more::Display;
use enumset::{EnumSetType, EnumSet, enum_set};
use tui::{
    Terminal,
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::Rect,
    widgets::{
        Block,
        BorderType,
        Borders,
        Widget,
    },
    style::{
        Style,
        self,
    },
};

type PBDF<O> = Pin<Box<dyn Future<Output=O>>>;
type PBDFR<O> = PBDF<Result<O, Error>>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Authentication Failure")]
    AuthenticationFailure,
}

pub trait Identity {}

pub enum Operation {
    File(FileOperation),
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Credentials {
    username: String,
    password: String,
}

#[derive(Debug, Display, PartialOrd, Ord, EnumSetType)]
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

#[derive(Debug, Display, PartialOrd, Ord, EnumSetType)]
pub enum UserOperation {
    CanCreateUsers,
    CanDeleteUsers,
    CanReadUsers,
    CanModifyUsers,
    CanGetUserInfo,
    CanDisconnectUsers,
    CannotBeDisconnected,
}

#[derive(Debug, Display, PartialOrd, Ord, EnumSetType)]
pub enum NewsOperation {
    ReadNews,
    PostNews,
}

#[derive(Debug, Display, PartialOrd, Ord, EnumSetType)]
pub enum ChatOperation {
    ReadChat,
    SendChat,
}

#[derive(Debug, Display, PartialOrd, Ord, EnumSetType)]
pub enum MiscOperation {
    CanUseAnyName,
    DontShowAgreement,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FilePermissions(EnumSet<FileOperation>);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UserPermissions(EnumSet<UserOperation>);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NewsPermissions(EnumSet<NewsOperation>);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChatPermissions(EnumSet<ChatOperation>);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MiscPermissions(EnumSet<MiscOperation>);

impl Default for FilePermissions {
    fn default() -> Self {
        Self(enum_set!(FileOperation::Download | FileOperation::UploadToDropbox))
    }
}

impl Default for UserPermissions {
    fn default() -> Self {
        Self(enum_set!(UserOperation::CanGetUserInfo))
    }
}

impl Default for NewsPermissions {
    fn default() -> Self {
        Self(enum_set!(NewsOperation::ReadNews))
    }
}

impl Default for ChatPermissions {
    fn default() -> Self {
        Self(enum_set!(ChatOperation::ReadChat | ChatOperation::SendChat))
    }
}

impl Default for MiscPermissions {
    fn default() -> Self {
        Self(enum_set!(MiscOperation::CanUseAnyName))
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct UserAccountPermissions {
    file: FilePermissions,
    user: UserPermissions,
    news: NewsPermissions,
    chat: ChatPermissions,
    misc: MiscPermissions,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct UserAccount {
    name: String,
    login: String,
    password: String,

    // permissions: UserAccountPermissions,

    // File Flags
    can_download_files: bool,
    can_upload_files: bool,
    can_upload_anywhere: bool,
    can_delete_files: bool,
    can_rename_files: bool,
    can_move_files: bool,
    can_comment_files: bool,
    can_create_folders: bool,
    can_delete_folders: bool,
    can_rename_folders: bool,
    can_move_folders: bool,
    can_comment_folders: bool,
    can_view_drop_boxes: bool,
    can_make_aliases: bool,

    // User Flags
    can_create_users: bool,
    can_delete_users: bool,
    can_read_users: bool,
    can_modify_users: bool,
    can_get_user_info: bool,
    can_disconnect_users: bool,
    cannot_be_disconnected: bool,

    // News Flags
    can_read_news: bool,
    can_post_news: bool,

    // Chat Flags
    can_read_chat: bool,
    can_send_chat: bool,

    // Misc. Flags
    can_use_any_name: bool,
    dont_show_agreement: bool,
}

impl UserAccount {
    // const INTERIOR_WIDTH: usize = 32;
    pub fn display(&self) {
        fn checkbox(value: bool) -> String {
            if value {
                "[*]"
            } else {
                "[ ]"
            }.into()
        }
        let al = Alignment::Left;
        let ar = Alignment::Right;
        let field_names = [
            ("Name", self.name.clone()),
            ("Login", self.login.clone()),
            ("Password", self.password.clone()),
            ("Can Download Files", checkbox(self.can_download_files)),
            ("Can Upload Files", checkbox(self.can_upload_files)),
            ("Can Upload Anywhere", checkbox(self.can_upload_anywhere)),
            ("Can Delete Files", checkbox(self.can_delete_files)),
            ("Can Rename Files", checkbox(self.can_rename_files)),
            ("Can Move Files", checkbox(self.can_move_files)),
            ("Can Comment Files", checkbox(self.can_comment_files)),
            ("Can Create Folders", checkbox(self.can_create_folders)),
            ("Can Delete Folders", checkbox(self.can_delete_folders)),
            ("Can Rename Folders", checkbox(self.can_rename_folders)),
            ("Can Move Folders", checkbox(self.can_move_folders)),
            ("Can Comment Folders", checkbox(self.can_comment_folders)),
            ("Can View Drop Boxes", checkbox(self.can_view_drop_boxes)),
            ("Can Make Aliases", checkbox(self.can_make_aliases)),
            ("Can Create Users", checkbox(self.can_create_users)),
            ("Can Delete Users", checkbox(self.can_delete_users)),
            ("Can Read Users", checkbox(self.can_read_users)),
            ("Can Modify Users", checkbox(self.can_modify_users)),
            ("Can Get User Info", checkbox(self.can_get_user_info)),
            ("Can Disconnect Users", checkbox(self.can_disconnect_users)),
            ("Cannot Be Disconnected", checkbox(self.cannot_be_disconnected)),
            ("Can Read News", checkbox(self.can_read_news)),
            ("Can Post News", checkbox(self.can_post_news)),
            ("Can Read Chat", checkbox(self.can_read_chat)),
            ("Can Send Chat", checkbox(self.can_send_chat)),
            ("Can Use Any Name", checkbox(self.can_use_any_name)),
            ("Don't Show Agreement", checkbox(self.dont_show_agreement)),
        ];
        let lw = field_names.iter().map(|s| s.0.len()).max().unwrap_or(1);
        let lr = field_names.iter().map(|s| s.1.len()).max().unwrap_or(1);
        println!("╭{}┬{}╮", pad_str_with("─User Account", lw, al, None, '─'), pad_str_with("",
                lr, al, None, '─'));
        for (label, value) in field_names {
            println!("│{}│{}│", pad_str_with(label, lw, al, None, ' '), pad_str_with(&value, lr, ar, None, ' '));
            println!("├{}┼{}┤", pad_str_with("", lw, al, None, '╌'), pad_str_with("", lr, al, None, '╌'));
        }
        // println!("│{}│", pad_str_with(&self.name, w, al, None, ' '));
        println!("╰{}┴{}╯", pad_str_with("", lw, ar, None, '─'), pad_str_with("",
                lr, al, None, '─'));
    }
}

struct Field {
}

struct Section {
}

struct Card {
    title: String,
    sections: Vec<Section>,
}

impl Card {
    fn render(&self) {
        let lw = 0;
        let lr = 0;
        let ar = Alignment::Right;
        let al = Alignment::Left;
        println!("╭{}┬{}╮", pad_str_with("", lw, ar, None, '─'), pad_str_with("", lr, al, None, '─'));
        // println!("├{}┤", pad_str_with("", w, al, None, '╌'));
        // println!("│{}│", pad_str_with(&self.name, w, al, None, ' '));
        println!("╰{}┴{}╯",
            pad_str_with("", lw, ar, None, '─'),
            pad_str_with("", lw, ar, None, '─'),
        )
    }
}

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

}
