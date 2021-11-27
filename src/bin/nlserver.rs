use tokio::{
    io::{
        AsyncRead,
        AsyncWrite,
        AsyncReadExt as _,
        AsyncWriteExt as _,
    },
    sync::watch,
    net::TcpListener,
};
use futures::stream::TryStreamExt;

use encoding::{
    Encoding,
    DecoderTrap,
    all::MAC_ROMAN,
    codec::singlebyte::SingleByteEncoding,
};

use std::path::PathBuf;

use anyhow::{anyhow, bail};

type Result<T> = anyhow::Result<T>;

use neolith::protocol::{
    HotlineProtocol,
    IntoFrameExt as _,
    ChatId,
    ChatSubject,
    ClientHandshakeRequest,
    GetFileInfo,
    GetFileInfoReply,
    GetFileNameList,
    GetFileNameListReply,
    FilePath,
    FileNameWithInfo,
    GetClientInfoTextRequest,
    GetClientInfoTextReply,
    GetMessages,
    GetMessagesReply,
    PostNews,
    NotifyNewsMessage,
    GetUserNameList,
    GetUserNameListReply,
    InviteToNewChat,
    InviteToNewChatReply,
    InviteToChat,
    JoinChat,
    JoinChatReply,
    LeaveChat,
    LoginReply,
    LoginRequest,
    SetChatSubject,
    Message,
    ProtocolVersion,
    ServerHandshakeReply,
    SendBroadcast,
    GenericReply,
    SendChat,
    SendInstantMessage,
    SendInstantMessageReply,
    ChatMessage,
    ServerMessage,
    SetClientUserInfo,
    NotifyUserChange,
    NotifyUserDelete,
    NotifyChatSubject,
    NotifyChatUserChange,
    NotifyChatUserDelete,
    TransactionFrame,
    UserId,
    UserNameWithInfo,
};

use neolith::server::{
    Broadcast,
    Chat,
    ChatRoomSubject,
    ChatRoomInvite,
    ChatRoomPresence,
    Event,
    InstantMessage,
    ServerEvents,
    User,
    bus::{Bus, Notification},
    files::{DirEntry, OsFiles, FileInfo},
    transaction_stream::Frames,
    users::{Users, UserList},
    chat::{Chats, ChatsService},
    news::News,
};

fn os_path(path: FilePath) -> PathBuf {
    match path {
        FilePath::Root => PathBuf::new(),
        FilePath::Directory(parts) => {
            let mut path = PathBuf::new();
            for part in parts {
                path.push(String::from_utf8(part).expect("bad path"));
            }
            path
        }
    }
}

struct Files;

impl Files {
    fn files() -> OsFiles {
        OsFiles::with_root("/tmp".into())
            .expect("bad root directory")
    }
    pub fn list(path: FilePath) -> Option<Vec<FileNameWithInfo>> {
        let tmp = Self::files();
        let path = os_path(path);
        let files = tmp.list(&path).ok()?;
        let files = files.into_iter()
            .map(Self::convert_direntry)
            .collect();
        Some(files)
    }
    pub fn info(path: FilePath) -> Option<FileInfo> {
        let tmp = Self::files();
        let path = os_path(path);
        let info = tmp.get_info(&path).ok()?;
        Some(info)
    }
    fn convert_direntry(entry: DirEntry) -> FileNameWithInfo {
        let DirEntry {
            creator_code,
            type_code,
            size,
            path,
            ..
        } = entry;
        let file_name = path.file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.as_bytes())
            .map(|b| b.to_vec())
            .unwrap_or(vec![]);
        FileNameWithInfo {
            file_name,
            file_size: (size as i32).into(),
            creator: (*creator_code.bytes()).into(),
            file_type: (*type_code.bytes()).into(),
            name_script: 0.into(),
        }
    }
}

#[derive(Debug, Clone)]
struct Globals {
    user_id: Option<UserId>,
    users: watch::Receiver<Users>,
    chats: watch::Receiver<Chats>,
    users_tx: UserList,
    chats_tx: ChatsService,
    bus: Bus,
    news: News<SingleByteEncoding>,
}

impl Globals {
    fn user(&self) -> Option<UserNameWithInfo> {
        self.user_id.and_then(|id| self.user_find(id))
    }
    fn require_user(&self) -> Result<UserNameWithInfo> {
        let user = self.user().ok_or(anyhow!("user unavailable"))?;
        Ok(user)
    }
    fn user_list(&mut self) -> Vec<UserNameWithInfo> {
        self.users.borrow().to_vec()
    }
    fn user_find(&self, id: UserId) -> Option<UserNameWithInfo> {
        self.users.borrow()
            .find(id)
            .cloned()
    }
    async fn user_add(&mut self, user: &UserNameWithInfo) {
        let user_id = self.users_tx.add(user.clone())
            .await
            .expect("failed to add user");
        self.user_id.replace(user_id);
        let user = self.require_user().unwrap();
        self.bus.publish(Notification::UserConnect(user.into()));
    }
    async fn user_update(&mut self, user: &UserNameWithInfo) {
        self.users_tx.update(user.clone())
            .await
            .expect("failed to update user");
        let user = self.require_user().unwrap();
        self.bus.publish(Notification::UserUpdate(user.into()));
    }
    async fn user_remove(&mut self, user: &UserNameWithInfo) {
        self.users_tx.delete(user.clone())
            .await
            .expect("failed to remove user");
        self.bus.publish(Notification::UserDisconnect(user.clone().into()));
    }
    fn chat_get_subject(&self, chat_id: ChatId) -> Option<ChatSubject> {
        let chats = self.chats.borrow();
        chats.room(chat_id)
            .cloned()
            .and_then(|room| room.subject)
            .map(ChatSubject::from)
    }
    fn chat_list(&self, chat_id: ChatId) -> Vec<UserNameWithInfo> {
        let users = self.users.borrow();
        let chats = self.chats.borrow();
        chats.room(chat_id)
            .into_iter()
            .flat_map(|r| r.users().into_iter())
            .map(|id| users.find(id))
            .flat_map(Option::into_iter)
            .cloned()
            .collect()
    }
    async fn chat_create(&mut self, creator: UserId, users: Vec<UserId>) -> ChatId {
        let chat_id = self.chats_tx.create(users.clone().into())
            .await
            .expect("failed to create chat room");
        let users = users.into_iter()
            .filter(|user| creator != *user);
        for user in users {
            self.bus.publish(
                Notification::ChatRoomInvite((chat_id, user).into())
            );
        }
        chat_id
    }
    async fn chat_invite(&mut self, chat_id: ChatId, user: UserId) {
        self.bus.publish(
            Notification::ChatRoomInvite((chat_id, user).into())
        );
    }
    async fn chat_join(&mut self, chat: ChatId, user: &UserNameWithInfo) {
        let presence = ChatRoomPresence::from((chat, user.clone().into()));
        self.chats_tx.join(presence.clone())
            .await
            .expect("failed to join chat room");
        self.bus.publish(Notification::ChatRoomJoin(presence));
    }
    async fn chat_leave(&mut self, chat: ChatId, user: &UserNameWithInfo) {
        let presence = ChatRoomPresence::from((chat, user.clone().into()));
        self.chats_tx.leave((chat, user.clone().into()).into())
            .await
            .expect("failed to leave chat room");
        self.bus.publish(Notification::ChatRoomLeave(presence));
    }
    async fn chat_subject_change(&mut self, chat: ChatId, subject: Vec<u8>) {
        let update = ChatRoomSubject::from((chat, subject));
        self.chats_tx.change_subject(update.clone())
            .await
            .expect("failed to update chat subject");
        self.bus.publish(Notification::ChatRoomSubjectUpdate(update));
    }
    fn chat(&mut self, chat: Chat) {
        let chat: ChatMessage = chat.into();
        self.bus.publish(chat.into());
    }
    fn instant_message(&mut self, message: InstantMessage) {
        let message = Notification::InstantMessage(message);
        self.bus.publish(message);
    }
    fn server_broadcast(&mut self, broadcast: Broadcast) {
        let broadcast = Notification::Broadcast(broadcast);
        self.bus.publish(broadcast);
    }
    fn post_news(&mut self, message: Message) {
        self.news.post(message.clone().into());
        let message: Vec<u8> = message.into();
        let news = Notification::News(message.into());
        self.bus.publish(news);
    }
}

#[tokio::main]
async fn main() -> Result<()> {

    let listener = TcpListener::bind("0.0.0.0:5500").await?;

    let bus = Bus::new();

    let (users_tx, users_rx) = UserList::new();
    let (chats_tx, chats_rx) = ChatsService::new();

    let mut news = News::new(*MAC_ROMAN);
    news.post(include_bytes!("../../neolith.txt").to_vec());

    let globals = Globals {
        user_id: None,
        users: users_rx.subscribe(),
        chats: chats_rx.subscribe(),
        users_tx,
        chats_tx,
        bus,
        news,
    };

    tokio::spawn(users_rx.run());
    tokio::spawn(chats_rx.run());

    loop {
        let (socket, addr) = listener.accept().await?;
        let (r, w) = socket.into_split();
        let mut conn = Connection::new(r, w, globals.clone());
        tokio::spawn(async move {
            while let Ok(_) = conn.process().await { }
            eprintln!("disconnect from {:?}", addr);
        });
    }

}

enum State<R, W> {
    New(New<R, W>),
    Unauthenticated(Unauthenticated<R, W>),
    Established(Established<R, W>),
    Closed,
    Borrowed,
}

impl <R: AsyncRead + Unpin, W: AsyncWrite + Unpin> State<R, W> {
    async fn process(&mut self) -> Result<()> {
        *self = match std::mem::replace(self, Self::Borrowed) {
            Self::Borrowed => {
                unreachable!("process() may not be called while borrowed")
            },
            Self::New(mut state) => {
                state.handshake().await?;
                let New(r, w, globals) = state;
                Self::Unauthenticated(Unauthenticated(r, w, globals))
            },
            Self::Unauthenticated(mut state) => {
                state.login().await?;
                let Unauthenticated(r, w, globals) = state;
                Self::Established(Established::new(r, w, globals))
            },
            Self::Established(state) => {
                state.handle().await?;
                Self::Closed
            },
            Self::Closed => Self::Closed,
        };
        Ok(())
    }
}

struct Connection<R, W> {
    state: State<R, W>,
}

impl <R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Connection<R, W> {
    fn new(r: R, w: W, globals: Globals) -> Self {
        Self {
            state: State::New(New(r, w, globals)),
        }
    }
    async fn process(&mut self) -> Result<()> {
        self.state.process().await
    }
}

struct New<R, W>(R, W, Globals);
impl <R: AsyncRead + Unpin, W: AsyncWrite + Unpin> New<R, W> {
    fn handshake_sync(buf: &[u8]) -> Result<ProtocolVersion> {
        match ClientHandshakeRequest::from_bytes(&buf) {
            Ok((_, _request)) => {
                Ok(123i16.into())
            },
            Err(e) => bail!("failed to parse handshake request: {:?}", e),
        }
    }
    pub async fn handshake(&mut self) -> Result<ProtocolVersion> {
        let Self(r, w, _) = self;

        let mut buf = [0u8; 12];
        r.read_exact(&mut buf).await?;
        let version = Self::handshake_sync(&buf)?;

        let reply = ServerHandshakeReply::ok();
        write_frame(w, reply).await?;

        Ok(version)
    }
}

struct Unauthenticated<R, W>(R, W, Globals);
impl <R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Unauthenticated<R, W> {
    pub async fn login(&mut self) -> Result<LoginRequest> {

        eprintln!("login attempt");

        let Self(r, w, globals) = self;

        let frame = Frames::new(r).next_frame().await?;

        let TransactionFrame { header, .. } = frame.clone();

        let login = LoginRequest::try_from(frame)?;

        let reply = LoginReply::default().reply_to(&header);
        write_frame(w, reply).await?;

        let LoginRequest { nickname, icon_id, .. } = &login;

        if let (
            Some(nickname),
            Some(icon_id),
        ) = (
            nickname.clone(),
            icon_id.clone(),
        ) {
            let user = UserNameWithInfo {
                icon_id,
                user_flags: 0.into(),
                username: nickname,
                user_id: 0.into(),
            };
            globals.user_add(&user).await;
        }

        Ok(login)
    }
}

struct Established<R, W> {
    r: R,
    w: W,
    globals: Globals,
}

impl <R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Established<R, W> {
    pub fn new(r: R, w: W, globals: Globals) -> Self {
        eprintln!("connection established");
        Self { r, w, globals }
    }
    pub async fn handle(mut self) -> Result<()> {
        match self.handle_inner().await {
            Ok(ok) => Ok(ok),
            Err(err) => {
                eprintln!("error");
                self.disconnect().await;
                Err(err)
            },
        }
    }
    async fn handle_inner(&mut self) -> Result<()> {
        let Self { r, w, globals } = self;
        let events = ServerEvents::new(r, globals.bus.subscribe())
            .events();
        let mut events = Box::pin(events);
        while let Some(event) = events.try_next().await? {
            match event {
                Event::Frame(frame) => Self::transaction(
                    w,
                    globals,
                    frame,
                ).await,
                Event::Notification(notification) => Self::notification(
                    w,
                    globals,
                    notification,
                ).await,
            }?;
        }
        Ok(())
    }
    async fn transaction(
        w: &mut W,
        globals: &mut Globals,
        frame: TransactionFrame,
    ) -> Result<()> {

        let TransactionFrame { header, body } = frame.clone();

        if let Ok(_) = GetUserNameList::try_from(frame.clone()) {
            eprintln!("get user name list");
            let reply = GetUserNameListReply::with_users(globals.user_list())
                .reply_to(&header);
            write_frame(w, reply).await?;
            return Ok(())
        }

        if let Ok(_) = GetMessages::try_from(frame.clone()) {
            eprintln!("get messages");
            let reply = GetMessagesReply::single(
                Message::new(globals.news.all())
            ).reply_to(&header);
            write_frame(w, reply).await?;
            return Ok(())
        }

        if let Ok(req) = PostNews::try_from(frame.clone()) {
            eprintln!("post news");
            let message = Message::from(req);
            let reply = GenericReply.reply_to(&header);
            write_frame(w, reply).await?;
            globals.post_news(message);
            return Ok(())
        }

        if let Ok(GetFileNameList(path)) = GetFileNameList::try_from(frame.clone()) {
            if let Some(p) = path.path() {
                let pathname: String = p.iter()
                    .map(|component| MAC_ROMAN.decode(&component, DecoderTrap::Replace).unwrap())
                    .collect::<Vec<String>>()
                    .join(":");
                eprintln!("get files: {:?}", &pathname);
            } else {
                eprintln!("get files: {:?}", &path);
            }
            let reply = GetFileNameListReply::with_files(
                Files::list(path).unwrap_or(vec![])
            ).reply_to(&header);
            write_frame(w, reply).await?;
            return Ok(())
        }

        if let Ok(info) = GetFileInfo::try_from(frame.clone()) {
            let GetFileInfo { path, filename } = info;
            let filename: Vec<u8> = filename.into();
            let path: Vec<Vec<u8>> = path.path()
                .unwrap_or_default()
                .to_vec()
                .into_iter()
                .chain(vec![filename.clone()].into_iter())
                .collect();
            let path = FilePath::Directory(path);
            let info = Files::info(path.clone())
                .expect(&format!("missing file: {:?}", &path));
            let reply: TransactionFrame = GetFileInfoReply {
                filename: filename.into(),
                size: (info.size as i32).into(),
                type_code: info.type_code.bytes().into(),
                creator: info.creator_code.bytes().to_vec().into(),
                comment: info.comment.as_bytes().to_vec().into(),
                created_at: info.modified_at.into(),
                modified_at: info.modified_at.into(),
            }.reply_to(&header);
            write_frame(w, reply).await?;
            return Ok(())
        }

        if let Ok(req) = SetClientUserInfo::try_from(frame.clone()) {
            if let Some(mut user) = globals.user() {
                user.username = req.username;
                user.icon_id = req.icon_id;
                globals.user_update(&user).await;
            } else {
                let SetClientUserInfo { username, icon_id } = req;
                let user = UserNameWithInfo {
                    icon_id,
                    username,
                    user_flags: 0.into(),
                    user_id: 0.into(),
                };
                globals.user_add(&user).await;
            }
            return Ok(())
        }

        if let Ok(req) = SendChat::try_from(frame.clone()) {
            let SendChat { chat_id, message, .. } = req;
            let user = globals.require_user()?;
            let chat = Chat(chat_id, user.clone().into(), message);
            globals.chat(chat);
            return Ok(())
        }

        if let Ok(req) = SendInstantMessage::try_from(frame.clone()) {
            let SendInstantMessage { user_id, message } = req;
            let user = globals.user();
            let to = globals.user_find(user_id);
            if let (Some(from), Some(to)) = (user, to) {
                let from = from.clone().into();
                let to = to.clone().into();
                let message = InstantMessage { from, to, message };
                globals.instant_message(message);
            }
            let reply = SendInstantMessageReply.reply_to(&header);
            write_frame(w, reply).await?;
            return Ok(())
        }

        if let Ok(req) = SendBroadcast::try_from(frame.clone()) {
            globals.server_broadcast(req.message.into());
            let reply = GenericReply.reply_to(&header);
            write_frame(w, reply).await?;
            return Ok(())
        }

        if let Ok(req) = InviteToNewChat::try_from(frame.clone()) {
            let user = globals.require_user()?.clone();
            let users = {
                let mut users: Vec<UserId> = req.into();
                users.push(user.user_id);
                users
            };
            eprintln!("users {:?}, ", &users);
            let chat_id = globals.chat_create(user.user_id, users).await;
            eprintln!("created {:?}, ", &chat_id);
            let reply = InviteToNewChatReply {
                chat_id,
                user_id: user.user_id,
                icon_id: user.icon_id,
                user_name: user.username,
                flags: user.user_flags,
            }.reply_to(&header);
            write_frame(w, reply).await?;
            return Ok(())
        }

        if let Ok(req) = InviteToChat::try_from(frame.clone()) {
            eprintln!("invite: {:?}", &req);
            let user = globals.require_user()?.clone();
            let InviteToChat { chat_id, user_id } = req;
            let reply = InviteToNewChatReply {
                chat_id,
                user_id,
                icon_id: user.icon_id,
                user_name: user.username,
                flags: user.user_flags,
            }.reply_to(&header);
            globals.chat_invite(chat_id, user_id).await;
            write_frame(w, reply).await?;
            return Ok(())
        }

        if let Ok(req) = JoinChat::try_from(frame.clone()) {
            eprintln!("join: {:?}", &req);
            let chat_id: ChatId = req.into();
            let user = globals.require_user()?;
            let subject = globals.chat_get_subject(chat_id);
            globals.chat_join(chat_id, &user).await;
            let users = globals.chat_list(chat_id);
            let reply = JoinChatReply::from((subject, users))
                .reply_to(&header);
            write_frame(w, reply).await?;
            return Ok(())
        }

        if let Ok(req) = LeaveChat::try_from(frame.clone()) {
            eprintln!("leave: {:?}", &req);
            let user = globals.require_user()?;
            globals.chat_leave(req.into(), &user).await;
            return Ok(())
        }

        if let Ok(req) = SetChatSubject::try_from(frame.clone()) {
            eprintln!("leave: {:?}", &req);
            let (chat_id, subject) = req.into();
            globals.chat_subject_change(chat_id, subject.into()).await;
            return Ok(())
        }

        if let Ok(req) = GetMessages::try_from(frame.clone()) {
            eprintln!("get messages: {:?}", &req);
            let reply = GetMessagesReply::empty().reply_to(&header);
            write_frame(w, reply).await?;
            return Ok(())
        }

        if let Ok(req) = GetClientInfoTextRequest::try_from(frame.clone()) {
            let GetClientInfoTextRequest { user_id } = req;
            let user = globals.user_find(user_id)
                .ok_or(anyhow!("could not find user"))?;
            let text = format!("{:#?}", &user).replace("\n", "\r");
            let reply = GetClientInfoTextReply {
                user_name: user.username,
                text: text.into_bytes(),
            }.reply_to(&header);
            write_frame(w, reply).await?;
            return Ok(())
        }

        eprintln!("established: unhandled request {:?} {:?}", header, body);

        Ok(())
    }
    async fn notification(
        w: &mut W,
        globals: &mut Globals,
        notification: Notification,
    ) -> Result<()> {
        let current_user = globals.user();
        match notification {
            Notification::Empty => {},
            Notification::Chat(chat) => {
                eprintln!("chat notification: {:?}", &chat);
                let chat: ChatMessage = chat.into();
                write_frame(w, chat.framed()).await?;
            },
            Notification::InstantMessage(message) => {
                let InstantMessage { from, to, message } = message;
                if current_user.map(|u| u.user_id) == Some(to.0.user_id) {
                    let message = ServerMessage {
                        user_id: Some(from.0.user_id),
                        user_name: Some(from.0.username),
                        message,
                    };
                    write_frame(w, message.framed()).await?;
                }
            },
            Notification::Broadcast(message) => {
                let broadcast: ServerMessage = message.into();
                write_frame(w, broadcast.framed()).await?;
            },
            Notification::News(article) => {
                let article: NotifyNewsMessage = article.into();
                write_frame(w, article.framed()).await?;
            }
            Notification::UserConnect(User(user))
            |
            Notification::UserUpdate(User(user)) => {
                let notify: NotifyUserChange = (&user).into();
                write_frame(w, notify.framed()).await?;
            },
            Notification::UserDisconnect(User(user)) => {
                let notify: NotifyUserDelete = (&user).into();
                write_frame(w, notify.framed()).await?;
            },
            Notification::ChatRoomInvite(ChatRoomInvite(chat_id, user)) => {
                if Some(user) == current_user.map(|u| u.user_id) {
                    let invite = InviteToChat {
                        user_id: user.into(),
                        chat_id,
                    };
                    write_frame(w, invite.framed()).await?;
                }
            },
            Notification::ChatRoomJoin(ChatRoomPresence(room, user)) => {
                let notify: NotifyChatUserChange = (room, &user.0).into();
                write_frame(w, notify.framed()).await?;
            },
            Notification::ChatRoomLeave(ChatRoomPresence(room, user)) => {
                let notify: NotifyChatUserDelete = (room, &user.0).into();
                write_frame(w, notify.framed()).await?;
            },
            Notification::ChatRoomSubjectUpdate(ChatRoomSubject(room, subject)) => {
                let notification = NotifyChatSubject::from(
                    (room, subject.into())
                );
                write_frame(w, notification.framed()).await?;
            },
        }
        Ok(())
    }
    async fn disconnect(&mut self) {
        eprintln!("disconnecting");
        let Self { globals, .. } = self;
        if let Some(user) = globals.user() {
            globals.user_remove(&user).await;
        } else {
            eprintln!("no user to remove");
        };
    }
}

async fn write_frame<W: AsyncWrite + Unpin, H: HotlineProtocol>(
    w: &mut W,
    h: H,
) -> Result<()> {
    w.write_all(&h.into_bytes()).await?;
    Ok(())
}
