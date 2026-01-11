use self::{
    bus::{Notification, Notifications},
    chat::{Chats, ChatsService},
    files::OsFiles,
    news::{News, NewsService},
    transaction_stream::Frames,
    transfers::TransfersService,
    users::{UserAccounts, Users, UsersService},
};
use crate::protocol::{
    self as proto, ChatId, ChatMessage, GenericReply, Message, NotifyNewsMessage, ProtocolError,
    ServerMessage, TransactionFrame, UserId, UserNameWithInfo,
};
use derive_more::{From, Into};
use encoding_rs::MACINTOSH;
use futures::stream::{Stream, StreamExt as _, TryStreamExt as _, select};
use std::path::PathBuf;
use thiserror::Error;
use tokio::{
    io::AsyncRead,
    sync::{
        broadcast::error::{RecvError, SendError},
        watch,
    },
};
use tracing::debug;

pub mod application;
pub mod bus;
pub mod chat;
pub mod files;
pub mod news;
pub mod transaction_stream;
pub mod transfers;
pub mod user_editor;
pub mod users;

#[derive(Debug, Error)]
pub enum BusError {
    #[error("dropped {0} messages from sender")]
    Lagged(u64),
    #[error("channel is closed")]
    Closed,
}

impl<T> From<SendError<T>> for BusError {
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
        let message = [&b"\r "[..], &username[..], &b": "[..], &text[..]].concat();
        ChatMessage { chat_id, message }
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
pub struct ChatRoomCreationRequest(pub UserId, pub Vec<UserId>);

#[derive(Debug, Clone, From, Into)]
pub struct ChatRoomPresence(pub ChatId, pub User);

#[derive(Debug, Clone, From, Into)]
pub struct ChatRoomLeave(pub ChatId, pub UserId);

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
pub struct DownloadInfo(pub proto::ReferenceNumber, pub proto::WaitingCount);

impl From<DownloadInfo> for proto::DownloadInfo {
    fn from(value: DownloadInfo) -> Self {
        let DownloadInfo(reference, waiting_count) = value;
        Self {
            reference,
            waiting_count,
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

impl<S: AsyncRead + Unpin> ServerEvents<S> {
    pub fn new(reader: S, notifications: Notifications) -> Self {
        Self {
            frames: Frames::new(reader),
            notifications,
        }
    }
    fn notifications(notifications: Notifications) -> impl Stream<Item = EventItem> {
        notifications.incoming().map(|n| Ok(Event::Notification(n)))
    }
    fn frames<F: AsyncRead + Unpin>(frames: Frames<F>) -> impl Stream<Item = EventItem> {
        frames
            .frames()
            .map_ok(Event::Frame)
            .map_err(ProtocolError::into)
    }
    pub fn events(self) -> impl Stream<Item = EventItem> {
        let Self {
            frames,
            notifications,
        } = self;
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
    SendInstantMessage(proto::SendInstantMessage),
    InviteToNewChat(proto::InviteToNewChat),
    InviteToChat(proto::InviteToChat),
    JoinChat(proto::JoinChat),
    LeaveChat(proto::LeaveChat),
    RejectChatInfo(proto::RejectChatInvite),
    SetChatSubject(proto::SetChatSubject),
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
    UserAccess,
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
    SendInstantMessageReply,
    JoinChatReply(proto::JoinChatReply),
    InviteToNewChatReply(proto::InviteToNewChatReply),
    NewFolderReply,
    SendBroadcastReply,
    SetUserReply,
    NewUserReply,
    DeleteUserReply,
    Rejected(Option<String>),
}

impl ServerResponse {
    fn reject(message: Option<String>) -> TransactionFrame {
        let mut frame = TransactionFrame::empty(proto::TransactionType::Error);
        frame.header.error_code = 1i32.into();
        if let Some(reason) = message {
            frame
                .body
                .parameters
                .push(proto::Parameter::new_error(reason));
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
            ServerResponse::NewFolderReply => GenericReply.into(),
            ServerResponse::GetUserReply(reply) => reply.into(),
            ServerResponse::Rejected(message) => ServerResponse::reject(message),
            ServerResponse::SetUserReply => GenericReply.into(),
            ServerResponse::NewUserReply => GenericReply.into(),
            ServerResponse::DeleteUserReply => GenericReply.into(),
            ServerResponse::SendBroadcastReply => GenericReply.into(),
            ServerResponse::SendInstantMessageReply => GenericReply.into(),
            ServerResponse::JoinChatReply(reply) => reply.into(),
            ServerResponse::InviteToNewChatReply(reply) => reply.into(),
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
    ChatRoomLeave(ChatRoomLeave),
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
    type Error = proto::ProtocolError;

    fn try_from(frame: TransactionFrame) -> Result<Self, Self::Error> {
        match frame.transaction_type()? {
            proto::TransactionType::GetMessages => {
                proto::GetMessages::try_from(frame).map(Into::into)
            }
            proto::TransactionType::PostNewsArticle => {
                proto::PostNews::try_from(frame).map(Into::into)
            }
            proto::TransactionType::GetFileNameList => {
                proto::GetFileNameList::try_from(frame).map(Into::into)
            }
            proto::TransactionType::Reply => todo!(),
            proto::TransactionType::Error => todo!(),
            proto::TransactionType::OldPostNews => proto::PostNews::try_from(frame).map(Into::into),
            proto::TransactionType::SendChat => proto::SendChat::try_from(frame).map(Into::into),
            proto::TransactionType::Login => todo!(),
            proto::TransactionType::SendInstantMessage => {
                proto::SendInstantMessage::try_from(frame).map(Into::into)
            }
            proto::TransactionType::DisconnectUser => {
                proto::DisconnectUser::try_from(frame).map(Into::into)
            }
            proto::TransactionType::InviteToNewChat => {
                proto::InviteToNewChat::try_from(frame).map(Into::into)
            }
            proto::TransactionType::InviteToChat => {
                proto::InviteToChat::try_from(frame).map(Into::into)
            }
            proto::TransactionType::RejectChatInvite => {
                proto::RejectChatInvite::try_from(frame).map(Into::into)
            }
            proto::TransactionType::JoinChat => proto::JoinChat::try_from(frame).map(Into::into),
            proto::TransactionType::LeaveChat => proto::LeaveChat::try_from(frame).map(Into::into),
            proto::TransactionType::SetChatSubject => {
                proto::SetChatSubject::try_from(frame).map(Into::into)
            }
            proto::TransactionType::Agreed => todo!(),
            proto::TransactionType::DownloadFile => {
                proto::DownloadFile::try_from(frame).map(Into::into)
            }
            proto::TransactionType::UploadFile => {
                proto::UploadFile::try_from(frame).map(Into::into)
            }
            proto::TransactionType::DeleteFile => {
                proto::DeleteFile::try_from(frame).map(Into::into)
            }
            proto::TransactionType::NewFolder => proto::NewFolder::try_from(frame).map(Into::into),
            proto::TransactionType::GetFileInfo => {
                proto::GetFileInfo::try_from(frame).map(Into::into)
            }
            proto::TransactionType::SetFileInfo => {
                proto::SetFileInfo::try_from(frame).map(Into::into)
            }
            proto::TransactionType::MoveFile => proto::MoveFile::try_from(frame).map(Into::into),
            proto::TransactionType::MakeFileAlias => {
                proto::MakeFileAlias::try_from(frame).map(Into::into)
            }
            proto::TransactionType::DownloadFolder => todo!(),
            proto::TransactionType::DownloadBanner => todo!(),
            proto::TransactionType::UploadFolder => todo!(),
            proto::TransactionType::GetUserNameList => {
                proto::GetUserNameList::try_from(frame).map(Into::into)
            }
            proto::TransactionType::GetClientInfoText => {
                proto::GetClientInfoText::try_from(frame).map(Into::into)
            }
            proto::TransactionType::SetClientUserInfo => {
                proto::SetClientUserInfo::try_from(frame).map(Into::into)
            }
            proto::TransactionType::NewUser => proto::NewUser::try_from(frame).map(Into::into),
            proto::TransactionType::DeleteUser => {
                proto::DeleteUser::try_from(frame).map(Into::into)
            }
            proto::TransactionType::GetUser => proto::GetUser::try_from(frame).map(Into::into),
            proto::TransactionType::SetUser => proto::SetUser::try_from(frame).map(Into::into),
            proto::TransactionType::UserBroadcast => {
                proto::SendBroadcast::try_from(frame).map(Into::into)
            }
            proto::TransactionType::GetNewsCategoryNameList => todo!(),
            proto::TransactionType::GetNewsArticleNameList => todo!(),
            proto::TransactionType::DeleteNewsItem => todo!(),
            proto::TransactionType::NewNewsFolder => todo!(),
            proto::TransactionType::NewNewsCategory => todo!(),
            proto::TransactionType::GetNewsArticleData => todo!(),
            proto::TransactionType::DeleteNewsArticle => todo!(),
            proto::TransactionType::ConnectionKeepAlive => todo!(),
            _ => Err(proto::ProtocolError::UnsupportedTransaction(
                frame.header.type_.into(),
            )),
        }
    }
}

#[derive(Debug)]
pub struct NeolithServer<TS: transfers::TransferStream> {
    user_id: proto::UserId,
    files: OsFiles,
    users: watch::Receiver<Users>,
    users_tx: UsersService,
    news: watch::Receiver<News>,
    news_tx: NewsService,
    chats: watch::Receiver<Chats>,
    chats_tx: ChatsService,
    transfers_tx: TransfersService<TS>,
    accounts: UserAccounts,
}

type ServerResult<T> = anyhow::Result<T>;

impl <TS: transfers::TransferStream + 'static> NeolithServer<TS> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        user_id: proto::UserId,
        files: OsFiles,
        accounts: UserAccounts,
        users: watch::Receiver<Users>,
        users_tx: UsersService,
        news: watch::Receiver<News>,
        news_tx: NewsService,
        chats: watch::Receiver<Chats>,
        chats_tx: ChatsService,
        transfers_tx: TransfersService<TS>,
    ) -> Self {
        Self {
            user_id,
            files,
            accounts,
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
    pub async fn handle_client<R: Into<ClientRequest>>(
        &mut self,
        request: R,
    ) -> ServerResult<Option<ServerResponse>> {
        let user = self.require_current_user()?;
        let span = tracing::Span::current();
        span.record("user_id", format!("{}", i16::from(user.user_id)));
        span.record("nick", format!("{}", user.username));
        match request.into() {
            ClientRequest::GetUserNameList(_) => Ok(Some(self.get_users().into())),
            ClientRequest::GetMessages(_) => Ok(Some(self.get_news().await.into())),
            ClientRequest::PostNews(req) => self.post_news(req.0).await.into(),
            ClientRequest::GetFileNameList(req) => {
                self.list_files(req.0).await.map(Into::into).map(Some)
            }
            ClientRequest::GetFileInfo(req) => self
                .file_info(req.path, req.filename)
                .await
                .map(Into::into)
                .map(Some),
            ClientRequest::SetClientUserInfo(req) => {
                self.set_user_info(req.username, req.icon_id).await?;
                Ok(None)
            }
            ClientRequest::GetClientInfoText(req) => self.get_user_info_text(req.user_id).map(Some),
            ClientRequest::SendChat(req) => {
                let proto::SendChat {
                    options,
                    chat_id,
                    message,
                } = req;
                if let Some(chat_id) = chat_id {
                    self.send_private_chat(options, chat_id, message).await?;
                } else {
                    self.send_chat(options, message).await?;
                }
                Ok(None)
            }
            ClientRequest::SendInstantMessage(proto::SendInstantMessage { user_id, message }) => {
                let from = user.clone().into();
                let to = self.get_user(user_id).map(Into::into);
                if let Some(to) = to {
                    let message = InstantMessage { from, to, message };
                    self.instant_message(message).await?;
                }
                Ok(Some(ServerResponse::SendInstantMessageReply))
            }
            ClientRequest::DownloadFile(req) => self
                .file_download(req.file_path, req.filename)
                .await
                .map(Some),
            ClientRequest::UploadFile(req) => self
                .file_upload(req.file_path, req.filename)
                .await
                .map(Some),
            ClientRequest::GetUser(proto::GetUser(login)) => {
                if let Some(account) = self.accounts.get(login).cloned() {
                    Ok(Some(ServerResponse::GetUserReply(account.try_into()?)))
                } else {
                    Ok(Some(ServerResponse::Rejected(Some(
                        "User account not found".to_string(),
                    ))))
                }
            }
            ClientRequest::SetUser(..) => Ok(Some(ServerResponse::SetUserReply)),
            ClientRequest::NewUser(..) => Ok(Some(ServerResponse::NewUserReply)),
            ClientRequest::DeleteUser(..) => Ok(Some(ServerResponse::DeleteUserReply)),
            ClientRequest::SendBroadcast(b) => {
                self.send_broadcast(b.message).await?;
                Ok(Some(ServerResponse::SendBroadcastReply))
            }
            ClientRequest::InviteToNewChat(req) => {
                let users: Vec<proto::UserId> = req.into();
                let user_id = user.user_id;
                let chat_id = self
                    .chats_tx
                    .create((user_id, users).into())
                    .await
                    .expect("failed to create chat room");
                let reply = proto::InviteToNewChatReply {
                    chat_id,
                    user_id,
                    icon_id: user.icon_id,
                    user_name: user.username,
                    flags: user.user_flags,
                };
                Ok(Some(ServerResponse::InviteToNewChatReply(reply)))
            }
            ClientRequest::InviteToChat(proto::InviteToChat { user_id, chat_id }) => {
                self.chats_tx
                    .invite((chat_id, user_id).into())
                    .await
                    .expect("failed to invite user");
                Ok(None)
            }
            ClientRequest::JoinChat(req) => {
                let chat_id = ChatId::from(req);
                let Some(chat_room) = self.get_chat_room(chat_id) else {
                    return Ok(Some(ServerResponse::Rejected(Some(
                        "invalid chat".to_string(),
                    ))));
                };
                let subject = chat_room.subject.clone().map(proto::ChatSubject::from);
                let users = chat_room.users();
                let users = users
                    .into_iter()
                    .map(|id| self.get_user(id))
                    .flat_map(Option::into_iter)
                    .collect::<Vec<_>>();
                self.join_chat(chat_id, user).await;
                let reply = proto::JoinChatReply::from((subject, users));

                Ok(Some(ServerResponse::JoinChatReply(reply)))
            }
            ClientRequest::LeaveChat(req) => {
                let chat_id = ChatId::from(req);
                if self.get_chat_room(chat_id).is_none() {
                    return Ok(Some(ServerResponse::Rejected(Some(
                        "invalid chat".to_string(),
                    ))));
                }
                self.leave_chat(chat_id, user.user_id).await;

                Ok(None)
            }
            ClientRequest::SetChatSubject(req) => {
                let (chat, subject) = req.into();
                let update = ChatRoomSubject(chat, subject.into());
                self.chats_tx
                    .change_subject(update)
                    .await
                    .expect("failed to update chat subject");
                Ok(None)
            }
            ClientRequest::NewFolder(req) => {
                let proto::NewFolder { path, filename } = req;
                Ok(Some(self.new_folder(&path, &filename).await))
            }
            _ => Ok(Some(ServerResponse::Rejected(Some("todo".to_string())))),
        }
    }
    fn get_users(&self) -> proto::GetUserNameListReply {
        let users = self.users.borrow().to_vec();
        proto::GetUserNameListReply::with_users(users)
    }
    fn get_user(&self, id: proto::UserId) -> Option<UserNameWithInfo> {
        let users = self.users.borrow();
        users.find(id).cloned()
    }
    fn get_user_info_text(&self, user_id: proto::UserId) -> ServerResult<ServerResponse> {
        let users = self.users.borrow();
        let user = users
            .find(user_id)
            .ok_or(anyhow::anyhow!("could not find user with id {user_id:?}"))?;
        let text = format!("{:#?}", &user).replace('\n', "\r");
        let (text, _, failed) = MACINTOSH.encode(&text);
        if failed {
            anyhow::bail!("failed to encode user info string");
        }
        let reply = proto::GetClientInfoTextReply {
            user_name: user.username.clone(),
            text: text.into_owned(),
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
        let files = self
            .files
            .list(&path)
            .await?
            .into_iter()
            .filter_map(|path| proto::FileNameWithInfo::try_from(path).ok())
            .collect::<Vec<_>>();
        Ok(proto::GetFileNameListReply::with_files(files))
    }
    async fn file_info(
        &self,
        path: proto::FilePath,
        name: proto::FileName,
    ) -> ServerResult<proto::GetFileInfoReply> {
        debug!("info {name:?} @ {path:?}");
        let path = PathBuf::from(path).join(PathBuf::from(&name));
        let info = self.files.get_info(&path).await?;
        let reply = proto::GetFileInfoReply {
            filename: name,
            size: info.total_size().try_into()?,
            type_code: proto::FileType::from(*info.file_type.bytes()),
            creator: info.creator.bytes().to_vec().into(),
            comment: info.comment.into(),
            created_at: info.created_at.into(),
            modified_at: info.modified_at.into(),
        };
        Ok(reply)
    }
    async fn new_folder(&mut self, path: &proto::FilePath, name: &proto::FileName) -> ServerResponse {
        let path = PathBuf::from(path.clone()).join(PathBuf::from(name));
        if let Err(e) = self.files.mkdir(&path).await {
            let msg =  e.to_string();
            return ServerResponse::Rejected(Some(msg))
        }
        ServerResponse::NewFolderReply
    }
    fn join_path(path: &proto::FilePath, name: &proto::FileName) -> PathBuf {
        let name_slice = [name.clone().into()];
        let path = path
            .path()
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
        let reply = self
            .transfers_tx
            .file_download(self.files.root(), path)
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
        let reply = self
            .transfers_tx
            .file_upload(self.files.root(), path)
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
            let user = proto::UserNameWithInfo::anonymous(nick, icon);
            self.users_tx.add(user).await?;
        }
        Ok(())
    }
    fn get_chat_room(&mut self, id: ChatId) -> Option<chat::ChatRoom> {
        let chats = self.chats.borrow();
        chats.room(id).cloned()
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
    async fn send_broadcast(&mut self, message: Vec<u8>) -> ServerResult<()> {
        self.chats_tx.broadcast(Broadcast(message)).await?;
        Ok(())
    }
    async fn instant_message(&mut self, message: InstantMessage) -> ServerResult<()> {
        self.chats_tx.instant_message(message).await?;
        Ok(())
    }
    fn current_user(&self) -> Option<UserNameWithInfo> {
        self.users.borrow().find(self.user_id).cloned()
    }
    async fn join_chat(&mut self, chat: ChatId, user: UserNameWithInfo) {
        let presence = ChatRoomPresence::from((chat, user.into()));
        self.chats_tx
            .join(presence)
            .await
            .expect("failed to join chat room");
    }
    async fn leave_chat(&mut self, chat: ChatId, user: UserId) {
        let presence = ChatRoomLeave::from((chat, user));
        self.chats_tx
            .leave(presence)
            .await
            .expect("failed to leave chat room");
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
            proto::FilePath::Directory(parts) => parts
                .iter()
                .map(|p| MACINTOSH.decode(p).0)
                .map(|p| p.to_string())
                .collect(),
        }
    }
}
