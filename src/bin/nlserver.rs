use anyhow::bail;
use derive_more::Into;
use encoding_rs::MACINTOSH;
use futures::stream::TryStreamExt;
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _},
    net::{TcpListener, TcpStream, ToSocketAddrs},
    sync::watch,
};
use tracing::{debug, instrument, trace, warn};
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

type Result<T> = anyhow::Result<T>;

use neolith::{
    protocol::{
        self as proto, ChatId, ClientHandshakeRequest, ConnectionKeepAlive, DownloadInfo,
        GenericReply, HotlineProtocol, IntoFrameExt as _, InviteToChat, LoginReply, LoginRequest,
        NotifyChatSubject, NotifyChatUserChange, NotifyChatUserDelete, NotifyNewsMessage,
        NotifyUserChange, NotifyUserDelete, ProtocolVersion, ServerHandshakeReply, ServerMessage,
        SetClientUserInfo, TransactionFrame, UserId, UserNameWithInfo,
    },
    server::{
        ChatRoomInvite, ChatRoomLeave, ChatRoomPresence, ChatRoomSubject, ClientRequest, Event,
        InstantMessage, NeolithServer, ServerEvents, User,
        bus::{Bus, Notification},
        chat::{Chats, ChatsService},
        files::OsFiles,
        news::{News, NewsService},
        transaction_stream::Frames,
        transfers::{TransferConnection, TransfersService},
        users::{UserAccounts, Users, UsersService},
    },
};

#[derive(Debug, Clone)]
struct Globals {
    user_id: Option<UserId>,
    users: watch::Receiver<Users>,
    chats: watch::Receiver<Chats>,
    news: watch::Receiver<News>,
    users_tx: UsersService,
    chats_tx: ChatsService,
    news_tx: NewsService,
    transfers_tx: TransfersService<TcpStream>,
    files: OsFiles,
    accounts: UserAccounts,
    bus: Bus,
    transaction_id: u32,
}

impl Globals {
    fn user(&self) -> Option<UserNameWithInfo> {
        self.user_id.and_then(|id| self.user_find(id))
    }
    fn user_find(&self, id: UserId) -> Option<UserNameWithInfo> {
        self.users.borrow().find(id).cloned()
    }
    async fn user_add(&mut self, user: &UserNameWithInfo) {
        let user_id = self
            .users_tx
            .add(user.clone())
            .await
            .expect("failed to add user");
        self.user_id.replace(user_id);
    }
    async fn user_remove(&mut self, user: &UserNameWithInfo) {
        self.users_tx
            .delete(user.clone())
            .await
            .expect("failed to remove user");
    }
    fn chat_list(&self, chat_id: ChatId) -> Vec<UserNameWithInfo> {
        let users = self.users.borrow();
        let chats = self.chats.borrow();
        chats
            .room(chat_id)
            .into_iter()
            .flat_map(|r| r.users().into_iter())
            .map(|id| users.find(id))
            .flat_map(Option::into_iter)
            .cloned()
            .collect()
    }
    async fn chat_remove(&mut self, user: &UserNameWithInfo) {
        let chats = self
            .chats_tx
            .leave_all(user.user_id)
            .await
            .expect("failed to leave all chat rooms");
        for chat in chats {
            let leave = ChatRoomLeave::from((chat, user.user_id));
            debug!("chat remove {leave:?}");
            self.bus.publish(Notification::ChatRoomLeave(leave));
        }
    }
    fn next_transaction_id(&mut self) -> proto::Id {
        let id = self.transaction_id;
        self.transaction_id += 1;
        proto::Id::from(id)
    }
    async fn disconnect(&mut self) {
        if let Some(user) = self.user() {
            self.chat_remove(&user).await;
            self.user_remove(&user).await;
        } else {
            debug!("no user to remove");
        };
    }
}

async fn new_listener_from_listenfd<A: ToSocketAddrs>(
    listenfd: &mut listenfd::ListenFd,
    fd: usize,
    fallback_addr: A,
) -> std::io::Result<TcpListener> {
    let listener = listenfd.take_tcp_listener(fd)?;
    if let Some(listener) = listener {
        listener.set_nonblocking(true)?;
        let listener = TcpListener::from_std(listener)?;
        Ok(listener)
    } else {
        let listener = TcpListener::bind(fallback_addr).await?;
        Ok(listener)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .try_init()?;

    let mut listenfd = listenfd::ListenFd::from_env();
    let host = "0.0.0.0";
    let listener = new_listener_from_listenfd(&mut listenfd, 0, (host, 5500)).await?;
    let transfer_listener = new_listener_from_listenfd(&mut listenfd, 1, (host, 5501)).await?;

    let bus = Bus::new();

    let (users_tx, users_rx) = UsersService::new(bus.clone());
    let (chats_tx, chats_rx) = ChatsService::new(bus.clone());
    let (news_tx, news_rx) = NewsService::new(MACINTOSH, bus.clone());
    let (transfers_tx, transfers_rx) = TransfersService::new(bus.clone());

    let files = OsFiles::with_root("files").await?;
    let accounts = UserAccounts::with_root("users").await?;

    let globals = Globals {
        user_id: None,
        users: users_rx.subscribe(),
        chats: chats_rx.subscribe(),
        news: news_rx.subscribe(),
        users_tx,
        chats_tx,
        news_tx,
        transfers_tx: transfers_tx.clone(),
        files: files.clone(),
        accounts,
        bus,
        transaction_id: 0,
    };

    tokio::spawn(transfers(transfer_listener, transfers_tx.clone()));
    tokio::spawn(users_rx.run());
    tokio::spawn(chats_rx.run());
    tokio::spawn(news_rx.run());
    tokio::spawn(transfers_rx.run());

    loop {
        let (socket, addr) = listener.accept().await?;
        let (r, w) = socket.into_split();
        let mut conn = Connection::new(r, w, globals.clone());
        tokio::task::spawn(async move {
            while conn.process().await.is_ok() {}
            debug!("disconnect from {:?}", addr);
        });
    }
}

#[instrument(skip(transfers_tx))]
async fn transfers(listener: TcpListener, transfers_tx: TransfersService<TcpStream>) -> Result<()> {
    loop {
        let (socket, _addr) = listener.accept().await?;
        let conn = TransferConnection::new(socket, transfers_tx.clone());
        tokio::spawn(conn.run());
    }
}

enum State<R, W> {
    New(New<R, W>),
    Unauthenticated(Unauthenticated<R, W>),
    Established(Established<R, W>),
    Closed,
    Borrowed,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> State<R, W> {
    async fn process(&mut self) -> Result<()> {
        *self = match std::mem::replace(self, Self::Borrowed) {
            Self::Borrowed => {
                unreachable!("process() may not be called while borrowed")
            }
            Self::New(mut state) => {
                state.handshake().await?;
                let New(r, w, globals) = state;
                Self::Unauthenticated(Unauthenticated(r, w, globals))
            }
            Self::Unauthenticated(mut state) => {
                state.login().await?;
                let Unauthenticated(r, w, globals) = state;
                Self::Established(Established::new(r, w, globals))
            }
            Self::Established(state) => {
                state.handle().await?;
                Self::Closed
            }
            Self::Closed => Self::Closed,
        };
        Ok(())
    }
}

impl<R, W> std::fmt::Debug for State<R, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::New(_) => write!(f, "New"),
            Self::Unauthenticated(_) => write!(f, "Unauthenticated"),
            Self::Established(_) => write!(f, "Established"),
            Self::Closed => write!(f, "Closed"),
            Self::Borrowed => write!(f, "Borrowed"),
        }
    }
}

struct Connection<R, W> {
    state: State<R, W>,
}

impl<R, W> std::fmt::Debug for Connection<R, W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.state)
    }
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Connection<R, W> {
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
impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> New<R, W> {
    fn handshake_sync(buf: &[u8]) -> Result<ProtocolVersion> {
        match ClientHandshakeRequest::try_from(buf) {
            Ok(_request) => Ok(123u16.into()),
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

#[derive(Debug, Into)]
struct VersionedLoginRequest(LoginRequest);

impl VersionedLoginRequest {
    fn old_style(&self) -> Option<(proto::Nickname, proto::IconId)> {
        let Self(req) = self;
        req.nickname.clone().zip(req.icon_id)
    }
    fn fill_in(&mut self, nickname: proto::Nickname, icon_id: proto::IconId) {
        let Self(req) = self;
        req.nickname = Some(nickname);
        req.icon_id = Some(icon_id);
    }
    fn login(&self) -> proto::UserLogin {
        let Self(req) = self;
        req.login
            .clone()
            .unwrap_or_else(proto::UserLogin::guest)
            .invert()
    }
    fn password(&self) -> proto::Password {
        let Self(req) = self;
        req.password.clone().unwrap_or_default()
    }
}

struct Unauthenticated<R, W>(R, W, Globals);
impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Unauthenticated<R, W> {
    pub async fn login(&mut self) -> Result<LoginRequest> {
        debug!("login attempt");

        let Self(r, w, globals) = self;

        let mut frames = Frames::new(r);

        let frame = frames.next_frame().await?;
        let TransactionFrame { header, .. } = frame;

        let mut request = VersionedLoginRequest(LoginRequest::try_from(frame)?);
        let login = request.login();
        let password = request.password();

        let Some(account) = globals.accounts.verify(login, password) else {
            anyhow::bail!("login failure");
        };

        debug!("login ok");

        let user_flags = proto::UserFlags {
            admin: account.is_admin(),
            ..Default::default()
        };

        let reply = LoginReply::default().reply_to(&header);
        write_frame(w, reply).await?;

        debug!("login request {request:?}");
        let user = if let Some((username, icon_id)) = request.old_style() {
            debug!("old login");
            UserNameWithInfo {
                icon_id,
                username_len: username.len() as u16,
                username,
                user_flags,
                user_id: 0.into(),
            }
        } else {
            debug!("new login, awaiting SetClientUserInfo");
            let frame = frames.next_frame().await?;
            let SetClientUserInfo { username, icon_id } = SetClientUserInfo::try_from(frame)?;
            request.fill_in(username.clone(), icon_id);
            UserNameWithInfo {
                icon_id,
                username_len: username.len() as u16,
                username,
                user_flags,
                user_id: 0.into(),
            }
        };
        debug!("adding user {user:?}");
        globals.user_add(&user).await;

        Ok(request.into())
    }
}

struct Established<R, W> {
    r: R,
    w: W,
    globals: Globals,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Established<R, W> {
    pub fn new(r: R, w: W, globals: Globals) -> Self {
        debug!("connection established");
        Self { r, w, globals }
    }
    #[instrument(fields(nick), skip(self))]
    pub async fn handle(mut self) -> Result<()> {
        match self.handle_inner().await {
            Ok(ok) => Ok(ok),
            Err(err) => {
                debug!("error: {:?}", &err);
                self.disconnect().await;
                Err(err)
            }
        }
    }
    async fn handle_inner(&mut self) -> Result<()> {
        let Self { r, w, globals } = self;
        let events = ServerEvents::new(r, globals.bus.subscribe()).events();
        let mut events = Box::pin(events);
        while let Some(event) = events.try_next().await? {
            match event {
                Event::Frame(frame) => Self::transaction(w, globals, frame).await,
                Event::Notification(notification) => {
                    Self::notification(w, globals, notification).await
                }
            }?;
        }
        Ok(())
    }
    async fn transaction(w: &mut W, globals: &mut Globals, frame: TransactionFrame) -> Result<()> {
        let TransactionFrame { header, body } = frame.clone();
        let mut server = NeolithServer::new(
            globals.user_id.unwrap_or_default(),
            globals.files.clone(),
            globals.accounts.clone(),
            globals.users.clone(),
            globals.users_tx.clone(),
            globals.news.clone(),
            globals.news_tx.clone(),
            globals.chats.clone(),
            globals.chats_tx.clone(),
            globals.transfers_tx.clone(),
        );

        let reply = if let Ok(req) = ClientRequest::try_from(frame.clone()) {
            trace!("auto decode using tryfrom: {req:?}");
            server
                .handle_client(req)
                .await?
                .map(|r| r.reply_to(&header))
        } else if ConnectionKeepAlive::try_from(frame.clone()).is_ok() {
            debug!("keep alive");
            Some(GenericReply.reply_to(&header))
        } else {
            warn!("established: unhandled request {:?} {:?}", header, body);
            None
        };

        trace!("processed message");

        if let Some(reply) = reply {
            trace!("replying with {reply:?}");
            write_frame(w, reply).await?;
            trace!("replied");
        }

        Ok(())
    }
    async fn notification(
        w: &mut W,
        globals: &mut Globals,
        notification: Notification,
    ) -> Result<()> {
        let current_user = globals.user();
        match notification {
            Notification::Empty => {}
            Notification::Chat(chat) => {
                let username = current_user.as_ref().map(|u| &u.username);
                if let Some(id) = chat.chat_id {
                    let chat_members = globals.chat_list(id);
                    debug!("chat {id:?} contains {chat_members:?}");
                    if let Some(user) = &current_user
                        && globals.chat_list(id).contains(user)
                    {
                        debug!("private chat notification -> {username:?}: {:?}", &chat);
                        Self::write_frame(w, globals, chat).await?;
                    }
                } else {
                    debug!("chat notification -> {username:?}: {:?}", &chat);
                    Self::write_frame(w, globals, chat).await?;
                }
            }
            Notification::InstantMessage(message) => {
                let InstantMessage { from, to, message } = message;
                if current_user.map(|u| u.user_id) == Some(to.0.user_id) {
                    let message = ServerMessage {
                        user_id: Some(from.0.user_id),
                        user_name: Some(from.0.username),
                        message,
                    };
                    Self::write_frame(w, globals, message).await?;
                }
            }
            Notification::Broadcast(message) => {
                let broadcast: ServerMessage = message.into();
                Self::write_frame(w, globals, broadcast).await?;
            }
            Notification::DownloadInfo(info) => {
                let info: DownloadInfo = info.into();
                Self::write_frame(w, globals, info).await?;
            }
            Notification::News(article) => {
                let article: NotifyNewsMessage = article.into();
                Self::write_frame(w, globals, article).await?;
            }
            Notification::UserConnect(User(user)) | Notification::UserUpdate(User(user)) => {
                let notify: NotifyUserChange = (&user).into();
                Self::write_frame(w, globals, notify).await?;
            }
            Notification::UserDisconnect(User(user)) => {
                let notify: NotifyUserDelete = (&user).into();
                Self::write_frame(w, globals, notify).await?;
            }
            Notification::ChatRoomInvite(ChatRoomInvite(chat_id, user_id)) => {
                if Some(user_id) == current_user.map(|u| u.user_id) {
                    let invite = InviteToChat { user_id, chat_id };
                    Self::write_frame(w, globals, invite).await?;
                }
            }
            Notification::ChatRoomJoin(ChatRoomPresence(room, user)) => {
                let notify: NotifyChatUserChange = (room, &user.0).into();
                Self::write_frame(w, globals, notify).await?;
            }
            Notification::ChatRoomLeave(ChatRoomLeave(room, user)) => {
                let notify: NotifyChatUserDelete = (room, user).into();
                Self::write_frame(w, globals, notify).await?;
            }
            Notification::ChatRoomSubjectUpdate(ChatRoomSubject(room, subject)) => {
                let notification = NotifyChatSubject::from((room, subject.into()));
                Self::write_frame(w, globals, notification).await?;
            }
        }
        Ok(())
    }
    async fn write_frame<F: proto::IntoFrameExt>(
        w: &mut W,
        globals: &mut Globals,
        f: F,
    ) -> Result<()> {
        let current_id = globals.next_transaction_id();
        let framed = f.framed().id(current_id);
        write_frame(w, framed).await?;
        Ok(())
    }
    async fn disconnect(&mut self) {
        debug!("disconnecting");
        let Self { globals, .. } = self;
        if let Some(user) = globals.user() {
            globals.chat_remove(&user).await;
            globals.user_remove(&user).await;
        } else {
            debug!("no user to remove");
        };
    }
}

async fn write_frame<W: AsyncWrite + Unpin, H: HotlineProtocol>(w: &mut W, h: H) -> Result<()> {
    w.write_all(&h.into_bytes()).await?;
    Ok(())
}
