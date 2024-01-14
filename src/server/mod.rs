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

#[derive(Debug, From)]
pub enum ClientRequest {
    GetMessages(proto::GetMessages),
    PostNews(proto::PostNews),
    GetFileNameList(proto::GetFileNameList),
    GetFileInfo(proto::GetFileInfo),
    SetFileInfo(proto::SetFileInfo),
    GetUserNameList(proto::GetUserNameList),
    GetClientInfoText(proto::GetClientInfoText),
    SetClientUserInfo(proto::SetClientUserInfo),
    DisconnectUser(proto::DisconnectUser),
    SendChat(proto::SendChat),
    DownloadFile(proto::DownloadFile),
    UploadFile(proto::UploadFile),
    DeleteFile(proto::DeleteFile),
    MoveFile(proto::MoveFile),
    NewFolder(proto::NewFolder),
    MakeFileAlias(proto::MakeFileAlias),
    NewUser(proto::NewUser),
    DeleteUser(proto::DeleteUser),
    GetUser(proto::GetUser),
    SetUser(proto::SetUser),
    SendBroadcast(proto::SendBroadcast),
}

#[derive(Debug, From)]
pub enum ServerResponse {
    GetUserNameListReply(proto::GetUserNameListReply),
    GetClientInfoTextReply(proto::GetClientInfoTextReply),
    GetMessagesReply(proto::GetMessagesReply),
    PostNewsReply,
    GetFileNameListReply(proto::GetFileNameListReply),
    GetFileInfoReply(proto::GetFileInfoReply),
    SetFileInfoReply(proto::SetFileInfoReply),
    DownloadFileReply(proto::DownloadFileReply),
    UploadFileReply(proto::UploadFileReply),
    DeleteFileReply(proto::DeleteFileReply),
    MoveFileReply(proto::MoveFileReply),
    GetUserReply(proto::GetUserReply),
    Rejected(Option<String>),
}

impl ServerResponse {
    fn reject(message: Option<String>) -> TransactionFrame {
        let mut frame = TransactionFrame::empty(proto::TransactionType::Error);
        frame.header.error_code = 1i32.into();
        if let Some(reason) = message {
            frame.body.parameters.push(proto::Parameter::new_error(reason));
        }
        frame
    }
}

impl From<ServerResponse> for TransactionFrame {
    fn from(val: ServerResponse) -> Self {
        match val {
            ServerResponse::GetUserNameListReply(reply) => reply.into(),
            ServerResponse::GetMessagesReply(reply) => reply.into(),
            ServerResponse::PostNewsReply => GenericReply.into(),
            ServerResponse::GetFileNameListReply(reply) => reply.into(),
            ServerResponse::GetFileInfoReply(reply) => reply.into(),
            ServerResponse::SetFileInfoReply(reply) => reply.into(),
            ServerResponse::GetClientInfoTextReply(reply) => reply.into(),
            ServerResponse::DownloadFileReply(reply) => reply.into(),
            ServerResponse::UploadFileReply(reply) => reply.into(),
            ServerResponse::DeleteFileReply(reply) => reply.into(),
            ServerResponse::MoveFileReply(reply) => reply.into(),
            ServerResponse::GetUserReply(reply) => reply.into(),
            ServerResponse::Rejected(message) => ServerResponse::reject(message),
        }
    }
}

impl From<ServerResponse> for ServerResult<Option<ServerResponse>> {
    fn from(val: ServerResponse) -> Self {
        Ok(Some(val))
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
        if let Ok(req) = proto::GetMessages::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::PostNews::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::GetFileNameList::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::GetFileInfo::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::SetFileInfo::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::GetUserNameList::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::GetClientInfoText::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::DisconnectUser::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::SendChat::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::DownloadFile::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::UploadFile::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::MoveFile::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::DeleteFile::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::NewFolder::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::MakeFileAlias::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::NewUser::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::DeleteUser::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::GetUser::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::SetUser::try_from(frame.clone()) {
            return Ok(req.into())
        }
        if let Ok(req) = proto::SendBroadcast::try_from(frame.clone()) {
            return Ok(req.into())
        }
        anyhow::bail!("invalid request")
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
            ClientRequest::GetUserNameList(_) => Ok(Some(self.get_users().await.into())),
            ClientRequest::GetMessages(_) => Ok(Some(self.get_news().await.into())),
            ClientRequest::PostNews(req) => self.post_news(req.0).await.into(),
            ClientRequest::GetFileNameList(req) => self.list_files(req.0)
                .await
                .map(Into::into)
                .map(Some),
            ClientRequest::GetFileInfo(req) => self.file_info(
                req.path,
                req.filename,
            )
                .await
                .map(Into::into)
                .map(Some),
            ClientRequest::SetFileInfo(_) => Ok(Some(proto::SetFileInfoReply.into())),
            ClientRequest::SetClientUserInfo(req) => {
                self.set_user_info(req.username, req.icon_id).await?;
                Ok(None)
            },
            ClientRequest::GetClientInfoText(req) => {
                self.get_user_info_text(req.user_id)
                    .await
                    .map(Some)
            },
            ClientRequest::SendChat(req) => {
                let proto::SendChat { options, chat_id, message } = req;
                if let Some(chat_id) = chat_id {
                    self.send_private_chat(options, chat_id, message).await?;
                } else {
                    self.send_chat(options, message).await?;
                }
                Ok(None)
            }
            ClientRequest::DownloadFile(req) => {
                self.file_download(req.file_path, req.filename).await.map(Some)
            },
            ClientRequest::UploadFile(req) => {
                self.file_upload(req.file_path, req.filename).await.map(Some)
            },
            ClientRequest::DeleteFile(_) => {
                Ok(Some(proto::DeleteFileReply.into()))
            },
            ClientRequest::MoveFile(_) => {
                Ok(Some(proto::MoveFileReply.into()))
            },
            _ => Ok(Some(ServerResponse::Rejected(Some("todo".to_string())))),
        }
    }
    async fn get_users(&self) -> proto::GetUserNameListReply {
        let users = self.users.borrow().to_vec();
        proto::GetUserNameListReply::with_users(users)
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
        Ok(reply.into())
    }
    async fn get_news(&self) -> proto::GetMessagesReply {
        let news = Message::new(self.news.borrow().all());
        debug!("{news:?}");
        proto::GetMessagesReply::single(news)
    }
    async fn post_news(&mut self, news: proto::Message) -> ServerResponse {
        debug!("post {news:?}");
        self.news_tx.post(news.into()).await;
        ServerResponse::PostNewsReply
    }
    async fn list_files(&self, path: proto::FilePath) -> ServerResult<proto::GetFileNameListReply> {
        debug!("list {path:?}");
        let path: PathBuf = path.into();
        let files = self.files()?;
        let files = files.list(&path)?
            .into_iter()
            .map(proto::FileNameWithInfo::from)
            .collect();
        Ok(proto::GetFileNameListReply::with_files(files))
    }
    async fn file_info(
        &self,
        path: proto::FilePath,
        name: proto::FileName,
    ) -> ServerResult<proto::GetFileInfoReply> {
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
        Ok(reply)
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
    ) -> ServerResult<ServerResponse> {
        let path = Self::join_path(&path, &name);
        let reply = self.transfers_tx.file_download(self.files_root.clone(), path)
            .await
            .ok_or_else(|| anyhow::anyhow!("failed to start download"))?;
        Ok(reply.into())
    }
    async fn file_upload(
        &mut self,
        path: proto::FilePath,
        name: proto::FileName,
    ) -> ServerResult<ServerResponse> {
        let path = Self::join_path(&path, &name);
        let reply = self.transfers_tx.file_upload(self.files_root.clone(), path)
            .await
            .ok_or_else(|| anyhow::anyhow!("failed to start upload"))?;
        Ok(reply.into())
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
