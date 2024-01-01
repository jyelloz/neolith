use std::path::PathBuf;
use encoding_rs::MACINTOSH;
use tokio::{
    io::AsyncRead,
    sync::{
        watch,
        broadcast::error::{SendError, RecvError},
    },
};
use futures::stream::{
    TryStreamExt as _,
    StreamExt as _,
    Stream,
    select,
};
use derive_more::{From, Into};
use thiserror::Error;
use tracing::debug;
use crate::protocol::{
    self as proto,
    ChatId,
    ChatMessage,
    Message,
    NotifyNewsMessage,
    ProtocolError,
    ServerMessage,
    TransactionFrame,
    UserId,
    UserNameWithInfo, GenericReply,
};
use self::{
    bus::{Notification, Notifications},
    chat::{Chats, ChatsService},
    files::OsFiles,
    news::{News, NewsService},
    transaction_stream::Frames,
    transfers::TransfersService,
    users::{UsersService, Users},
};

pub mod application;
pub mod bus;
pub mod files;
pub mod users;
pub mod user_editor;
pub mod chat;
pub mod news;
pub mod transaction_stream;
pub mod transfers;

#[derive(Debug, Error)]
pub enum BusError {
    #[error("dropped {0} messages from sender")]
    Lagged(u64),
    #[error("channel is closed")]
    Closed,
}

impl <T> From<SendError<T>> for BusError {
    fn from(_: SendError<T>) -> Self {
        Self::Closed
    }
}

impl From<RecvError> for BusError {
    fn from(error: RecvError) -> Self {
        match error {
            RecvError::Closed => Self::Closed,
            RecvError::Lagged(n) => Self::Lagged(n),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Chat(pub Option<ChatId>, pub User, pub Vec<u8>);

impl From<Chat> for ChatMessage {
    fn from(val: Chat) -> Self {
        let Chat(chat_id, user, text) = val;
        let username = user.0.username.take();
        let message = [
            &b"\r "[..],
            &username[..],
            &b": "[..],
            &text[..],
        ].concat();
        ChatMessage {
            chat_id,
            message,
        }
    }
}

#[derive(Debug, Clone, From, Into)]
pub struct User(pub UserNameWithInfo);

impl From<User> for UserId {
    fn from(val: User) -> Self {
        val.0.user_id
    }
}

#[derive(Debug, Clone, From, Into)]
pub struct ChatRoomSubject(pub ChatId, pub Vec<u8>);

#[derive(Debug, Clone, From, Into)]
pub struct ChatRoomCreationRequest(pub Vec<UserId>);

#[derive(Debug, Clone, From, Into)]
pub struct ChatRoomPresence(pub ChatId, pub User);

#[derive(Debug, Clone, From, Into)]
pub struct ChatRoomInvite(pub ChatId, pub UserId);

#[derive(Debug, Clone)]
pub struct InstantMessage {
    pub from: User,
    pub to: User,
    pub message: Vec<u8>,
}

pub type BusResult<T> = Result<T, BusError>;

#[derive(Debug, Clone, From, Into)]
pub struct Broadcast(pub Vec<u8>);

impl From<Broadcast> for ServerMessage {
    fn from(val: Broadcast) -> Self {
        let Broadcast(message) = val;
        ServerMessage {
            message,
            user_id: None,
            user_name: None,
        }
    }
}

#[derive(Debug, Clone, From, Into)]
pub struct Article(pub Vec<u8>);

impl From<Article> for NotifyNewsMessage {
    fn from(val: Article) -> Self {
        let Article(mut message) = val;
        message.extend_from_slice(news::SEPARATOR.as_bytes());
        let message = Message::from(message);
        NotifyNewsMessage::from(message)
    }
}

pub enum Event {
    Notification(Notification),
    Frame(TransactionFrame),
}

#[derive(Debug, Error)]
pub enum EventError {
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}

pub struct ServerEvents<S> {
    frames: Frames<S>,
    notifications: Notifications,
}

type EventItem = Result<Event, EventError>;

impl <S: AsyncRead + Unpin> ServerEvents<S> {
    pub fn new(reader: S, notifications: Notifications) -> Self {
        Self {
            frames: Frames::new(reader),
            notifications,
        }
    }
    fn notifications(notifications: Notifications) -> impl Stream<Item = EventItem> {
        notifications.incoming()
            .map(|n| Ok(Event::Notification(n)))
    }
    fn frames<F: AsyncRead + Unpin>(frames: Frames<F>) -> impl Stream<Item = EventItem> {
        frames.frames()
            .map_ok(Event::Frame)
            .map_err(ProtocolError::into)
    }
    pub fn events(self) -> impl Stream<Item = EventItem> {
        let Self { frames, notifications } = self;
        let frames = Self::frames(frames);
        let notifications = Self::notifications(notifications);
        select(frames, notifications)
    }
}

#[derive(Debug)]
pub enum ClientRequest {
    GetMessages,
    PostNews(proto::Message),
    GetFileNameList(proto::FilePath),
    GetFileInfo(proto::FilePath, proto::FileName),
    GetUserNameList,
    GetClientInfoText(proto::UserId),
    SetClientUserInfo(proto::Nickname, proto::IconId),
    SendChat(proto::ChatOptions, Option<proto::ChatId>, Vec<u8>),
    DownloadFile(proto::FilePath, proto::FileName),
    UploadFile(proto::FilePath, proto::FileName),
}

impl From<proto::GetMessages> for ClientRequest {
    fn from(_: proto::GetMessages) -> Self {
        Self::GetUserNameList
    }
}

impl From<proto::GetUserNameList> for ClientRequest {
    fn from(_: proto::GetUserNameList) -> Self {
        Self::GetUserNameList
    }
}

impl From<proto::PostNews> for ClientRequest {
    fn from(val: proto::PostNews) -> Self {
        Self::PostNews(val.into())
    }
}

impl From<proto::GetClientInfoTextRequest> for ClientRequest {
    fn from(val: proto::GetClientInfoTextRequest) -> Self {
        Self::GetClientInfoText(val.user_id)
    }
}

impl From<proto::SetClientUserInfo> for ClientRequest {
    fn from(val: proto::SetClientUserInfo) -> Self {
        Self::SetClientUserInfo(val.username, val.icon_id)
    }
}

impl From<proto::SendChat> for ClientRequest {
    fn from(val: proto::SendChat) -> Self {
        Self::SendChat(val.options, val.chat_id, val.message)
    }
}

impl From<proto::DownloadFile> for ClientRequest {
    fn from(val: proto::DownloadFile) -> Self {
        Self::DownloadFile(val.file_path, val.filename)
    }
}

impl From<proto::UploadFile> for ClientRequest {
    fn from(val: proto::UploadFile) -> Self {
        Self::UploadFile(val.file_path, val.filename)
    }
}

#[derive(Debug)]
pub enum ServerResponse {
    GetUserNameListReply(proto::GetUserNameListReply),
    GetClientInfoTextReply(proto::GetClientInfoTextReply),
    GetMessagesReply(proto::GetMessagesReply),
    PostNewsReply,
    GetFileNameListReply(proto::GetFileNameListReply),
    GetFileInfoReply(proto::GetFileInfoReply),
    DownloadFileReply(proto::DownloadFileReply),
    UploadFileReply(proto::UploadFileReply),
}

impl From<ServerResponse> for TransactionFrame {
    fn from(val: ServerResponse) -> Self {
        match val {
            ServerResponse::GetUserNameListReply(reply) => reply.into(),
            ServerResponse::GetMessagesReply(reply) => reply.into(),
            ServerResponse::PostNewsReply => GenericReply.into(),
            ServerResponse::GetFileNameListReply(reply) => reply.into(),
            ServerResponse::GetFileInfoReply(reply) => reply.into(),
            ServerResponse::GetClientInfoTextReply(reply) => reply.into(),
            ServerResponse::DownloadFileReply(reply) => reply.into(),
            ServerResponse::UploadFileReply(reply) => reply.into(),
        }
    }
}

#[derive(Debug)]
pub enum ServerRequest {
    Empty,
    Chat(ChatMessage),
    ChatRoomSubjectUpdate(ChatRoomSubject),
    ChatRoomInvite(ChatRoomInvite),
    ChatRoomJoin(ChatRoomPresence),
    ChatRoomLeave(ChatRoomPresence),
    Broadcast(Broadcast),
    News(Article),
    InstantMessage(InstantMessage),
    UserConnect(User),
    UserUpdate(User),
    UserDisconnect(User),
}

pub enum ClientResponse {
    RejectChatInvite,
}

impl TryFrom<TransactionFrame> for ClientRequest {
    type Error = anyhow::Error;

    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        if proto::GetUserNameList::try_from(frame).is_ok() {
            Ok(Self::GetUserNameList)
        } else {
            anyhow::bail!("invalid request")
        }
    }
}

#[derive(Debug)]
pub struct NeolithServer {
    user_id: proto::UserId,
    files_root: PathBuf,
    users: watch::Receiver<Users>,
    users_tx: UsersService,
    news: watch::Receiver<News>,
    news_tx: NewsService,
    chats: watch::Receiver<Chats>,
    chats_tx: ChatsService,
    transfers_tx: TransfersService,
}

type ServerResult<T> = anyhow::Result<T>;

impl NeolithServer {
    #[allow(clippy::too_many_arguments)]
    pub fn new<P: Into<PathBuf>>(
        user_id: proto::UserId,
        files_root: P,
        users: watch::Receiver<Users>,
        users_tx: UsersService,
        news: watch::Receiver<News>,
        news_tx: NewsService,
        chats: watch::Receiver<Chats>,
        chats_tx: ChatsService,
        transfers_tx: TransfersService,
    ) -> Self {
        Self {
            user_id,
            files_root: files_root.into(),
            users,
            users_tx,
            news,
            news_tx,
            chats,
            chats_tx,
            transfers_tx,
        }
    }
    #[tracing::instrument(fields(user_id, nick), skip(self, request))]
    pub async fn handle_client<R: Into<ClientRequest>>(&mut self, request: R) -> ServerResult<Option<ServerResponse>> {
        let user = self.require_current_user()?;
        let span = tracing::Span::current();
        span.record("user_id", format!("{}", i16::from(user.user_id)));
        span.record("nick", format!("{}", user.username));
        match request.into() {
            ClientRequest::GetUserNameList => Ok(Some(self.get_users().await)),
            ClientRequest::GetMessages => Ok(Some(self.get_news().await)),
            ClientRequest::PostNews(news) => Ok(Some(self.post_news(news).await)),
            ClientRequest::GetFileNameList(path) => self.list_files(path).await,
            ClientRequest::GetFileInfo(path, name) => self.file_info(path, name).await,
            ClientRequest::SetClientUserInfo(nick, icon) => {
                self.set_user_info(nick, icon).await?;
                Ok(None)
            },
            ClientRequest::GetClientInfoText(user_id) => self.get_user_info_text(user_id)
                .await
                .map(Some),
            ClientRequest::SendChat(options, id, message) => {
                if let Some(chat_id) = id {
                    self.send_private_chat(options, chat_id, message).await?;
                } else {
                    self.send_chat(options, message).await?;
                }
                Ok(None)
            }
            ClientRequest::DownloadFile(path, name) => self.file_download(path, name).await,
            ClientRequest::UploadFile(path, name) => self.file_upload(path, name).await,
        }
    }
    async fn get_users(&self) -> ServerResponse {
        let users = self.users.borrow().to_vec();
        ServerResponse::GetUserNameListReply(proto::GetUserNameListReply::with_users(users))
    }
    async fn get_user_info_text(&self, user_id: proto::UserId) -> ServerResult<ServerResponse> {
        let users = self.users.borrow();
        let user = users.find(user_id)
            .ok_or(anyhow::anyhow!("could not find user with id {user_id:?}"))?;
        let text = format!("{:#?}", &user).replace('\n', "\r");
        let reply = proto::GetClientInfoTextReply {
            user_name: user.username.clone(),
            text: text.into_bytes(),
        };
        Ok(ServerResponse::GetClientInfoTextReply(reply))
    }
    async fn get_news(&self) -> ServerResponse {
        let news = Message::new(self.news.borrow().all());
        debug!("{news:?}");
        ServerResponse::GetMessagesReply(proto::GetMessagesReply::single(news))
    }
    async fn post_news(&mut self, news: proto::Message) -> ServerResponse {
        debug!("post {news:?}");
        self.news_tx.post(news.into()).await;
        ServerResponse::PostNewsReply
    }
    async fn list_files(&self, path: proto::FilePath) -> ServerResult<Option<ServerResponse>> {
        debug!("list {path:?}");
        let path: PathBuf = path.into();
        let files = self.files()?;
        let files = files.list(&path)?
            .into_iter()
            .map(proto::FileNameWithInfo::from)
            .collect();
        let reply = proto::GetFileNameListReply::with_files(files);
        Ok(Some(ServerResponse::GetFileNameListReply(reply)))
    }
    async fn file_info(
        &self,
        path: proto::FilePath,
        name: proto::FileName,
    ) -> ServerResult<Option<ServerResponse>> {
        debug!("info {name:?} @ {path:?}");
        let path = PathBuf::from(path).join(PathBuf::from(&name));
        let files = self.files()?;
        let info = files.get_info(&path)?;
        let reply = proto::GetFileInfoReply {
            filename: name,
            size: (info.size as i32).into(),
            type_code: info.type_code.bytes().into(),
            creator: info.creator_code.bytes().to_vec().into(),
            comment: info.comment.as_bytes().to_vec().into(),
            created_at: info.created_at.into(),
            modified_at: info.modified_at.into(),
        };
        Ok(Some(ServerResponse::GetFileInfoReply(reply)))
    }
    fn join_path(path: &proto::FilePath, name: &proto::FileName) -> PathBuf {
        let name_slice = [name.clone().into()];
        let path = path.path()
            .into_iter()
            .flat_map(|p| p.iter())
            .chain(name_slice.iter())
            .map(|p| MACINTOSH.decode(p).0.to_string());
        PathBuf::from_iter(path)
    }
    async fn file_download(
        &mut self,
        path: proto::FilePath,
        name: proto::FileName,
    ) -> ServerResult<Option<ServerResponse>> {
        let path = Self::join_path(&path, &name);
        let reply = self.transfers_tx.file_download(self.files_root.clone(), path)
            .await
            .ok_or_else(|| anyhow::anyhow!("failed start download"))?;
        Ok(Some(ServerResponse::DownloadFileReply(reply)))
    }
    async fn file_upload(
        &mut self,
        path: proto::FilePath,
        name: proto::FileName,
    ) -> ServerResult<Option<ServerResponse>> {
        let path = Self::join_path(&path, &name);
        let reply = self.transfers_tx.file_upload(self.files_root.clone(), path)
            .await
            .ok_or_else(|| anyhow::anyhow!("failed start upload"))?;
        Ok(Some(ServerResponse::UploadFileReply(reply)))
    }
    async fn set_user_info(
        &mut self,
        nick: proto::Nickname,
        icon: proto::IconId,
    ) -> ServerResult<()> {
        debug!("set user info {nick:?}, {icon:?}");
        if let Some(mut user) = self.current_user() {
            user.username = nick;
            user.icon_id = icon;
            self.users_tx.update(user).await?;
        } else {
            let user = proto::UserNameWithInfo::anonymous(
                nick,
                icon,
            );
            self.users_tx.add(user).await?;
        }
        Ok(())
    }
    async fn send_chat(
        &mut self,
        _options: proto::ChatOptions,
        message: Vec<u8>,
    ) -> ServerResult<()> {
        let user = self.require_current_user()?;
        let chat = Chat(None, user.into(), message);
        self.chats_tx.chat(chat.into()).await?;
        Ok(())
    }
    async fn send_private_chat(
        &mut self,
        _options: proto::ChatOptions,
        chat_id: proto::ChatId,
        message: Vec<u8>,
    ) -> ServerResult<()> {
        let user = self.require_current_user()?;
        let chat = Chat(Some(chat_id), user.into(), message);
        self.chats_tx.chat(chat.into()).await?;
        Ok(())
    }
    fn files(&self) -> ServerResult<OsFiles> {
        Ok(OsFiles::with_root(&self.files_root)?)
    }
    fn current_user(&self) -> Option<UserNameWithInfo> {
        self.users.borrow()
            .find(self.user_id)
            .cloned()
    }
    fn require_current_user(&self) -> ServerResult<UserNameWithInfo> {
        self.current_user()
            .ok_or_else(|| anyhow::anyhow!("no current user"))
    }
    pub async fn handle_server(
        &mut self,
        _: ServerRequest,
    ) -> ServerResult<Option<ClientResponse>> {
        todo!();
    }
}

impl From<proto::FilePath> for PathBuf {
    fn from(value: proto::FilePath) -> Self {
        match value {
            proto::FilePath::Root => PathBuf::new(),
            proto::FilePath::Directory(parts) => parts.iter()
                .map(|p| MACINTOSH.decode(p).0)
                .map(|p| p.to_string())
                .collect(),
        }
    }
}
