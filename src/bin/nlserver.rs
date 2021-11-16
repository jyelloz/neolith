use tokio::{
    io::{
        AsyncRead,
        AsyncWrite,
        AsyncReadExt as _,
        AsyncWriteExt as _,
    },
    net::TcpListener,
};
use futures::stream::TryStreamExt;

use encoding::{
    Encoding,
    DecoderTrap,
    all::MAC_ROMAN,
};

use std::{
    sync::{Arc, Mutex},
    path::PathBuf,
};

use anyhow::bail;

type Result<T> = anyhow::Result<T>;

use neolith::protocol::{
    HotlineProtocol,
    IntoFrameExt as _,
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
    GetUserNameList,
    GetUserNameListReply,
    LoginReply,
    LoginRequest,
    Message,
    ProtocolVersion,
    ServerHandshakeReply,
    SendBroadcast,
    SendBroadcastReply,
    SendChat,
    SendInstantMessage,
    SendInstantMessageReply,
    ChatMessage,
    ServerMessage,
    SetClientUserInfo,
    NotifyUserChange,
    NotifyUserDelete,
    TransactionFrame,
    UserId,
    UserNameWithInfo,
};

use neolith::server::{
    Bus,
    User,
    Chat,
    InstantMessage,
    Message as BusMessage,
    ServerEvents,
    files::{DirEntry, OsFiles, FileInfo},
    transaction_stream::Frames,
    transaction_sink::Frames as FramesSink,
    users::Users,
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
    user: Option<UserNameWithInfo>,
    users: Arc<Mutex<Users>>,
    bus: Bus,
}

impl Globals {
    fn user_list(&mut self) -> Vec<UserNameWithInfo> {
        if let Ok(users) = self.users.lock() {
            users.to_vec()
        } else {
            vec![]
        }
    }
    fn user_find(&self, id: UserId) -> Option<UserNameWithInfo> {
        self.users.lock()
            .ok()
            .and_then(|users| users.find(id).map(|u| u.clone()))
    }
    fn user_add(&mut self, user: &mut UserNameWithInfo) -> bool {
        let Self { users, bus, .. } = self;
        if let Ok(mut users) = users.lock() {
            users.add(user);
            bus.user_connect(user.clone().into());
            true
        } else {
            false
        }
    }
    fn user_update(&mut self, user: &UserNameWithInfo) -> bool {
        let Self { users, bus, .. } = self;
        if let Ok(mut users) = users.lock() {
            users.update(user);
            bus.user_update(user.clone().into()).map(|_| true).unwrap_or(false)
        } else {
            false
        }
    }
    fn user_remove(&mut self, user: &UserNameWithInfo) {
        let Self { users, bus, .. } = self;
        if let Ok(mut users) = users.lock() {
            users.remove(user);
            bus.user_disconnect(user.clone().into());
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {

    let listener = TcpListener::bind("0.0.0.0:5500").await?;

    let globals = Globals {
        user: None,
        users: Arc::new(Mutex::new(Users::new())),
        bus: Bus::new(10),
    };

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
            Ok((_, request)) => {
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

        dbg!(&frame);

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
            let mut user = UserNameWithInfo {
                icon_id,
                user_flags: 0.into(),
                username: nickname,
                user_id: 0.into(),
            };
            globals.user_add(&mut user);
            globals.user = Some(user.clone());
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
                self.disconnect();
                Err(err)
            },
        }
    }
    async fn handle_inner(&mut self) -> Result<()> {
        let Self { r, w, globals } = self;
        let events = ServerEvents::new(
            r,
            globals.bus.clone(),
        ).events();
        let mut events = Box::pin(events);
        while let Some(event) = events.try_next().await? {
            let current_user = &globals.user;
            match event {
                BusMessage::TransactionReceived(frame) => Self::transaction(
                    w,
                    globals,
                    frame,
                ).await?,
                BusMessage::Chat(chat) => {
                    let chat: ChatMessage = chat.into();
                    write_frame(w, chat.framed()).await?;
                },
                BusMessage::InstantMessage(message) => {
                    let InstantMessage { from, to, message } = message;
                    if current_user.as_ref().map(|u| u.user_id.clone()) == Some(to.0.user_id) {
                        let message = ServerMessage {
                            user_id: Some(from.0.user_id),
                            user_name: Some(from.0.username),
                            message,
                        };
                        write_frame(w, message.framed()).await?;
                    }
                },
                BusMessage::Broadcast(message) => {
                    let broadcast: ServerMessage = message.into();
                    write_frame(w, broadcast.framed()).await?;
                },
                BusMessage::UserConnect(User(user))
                |
                BusMessage::UserUpdate(User(user)) => {
                    let notify: NotifyUserChange = (&user).into();
                    write_frame(w, notify.framed()).await?;
                },
                BusMessage::UserDisconnect(User(user)) => {
                    let notify: NotifyUserDelete = (&user).into();
                    write_frame(w, notify.framed()).await?;
                },
            }
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
                Message::new(include_bytes!("../../neolith.txt").to_vec())
            ).reply_to(&header);
            write_frame(w, reply).await?;
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
            let user = if let Some(mut user) = globals.user.clone() {
                user.username = req.username;
                user.icon_id = req.icon_id;
                globals.user_update(&user);
                user
            } else {
                let SetClientUserInfo { username, icon_id } = req;
                let mut user = UserNameWithInfo {
                    icon_id,
                    username,
                    user_flags: 0.into(),
                    user_id: 0.into(),
                };
                globals.user_add(&mut user);
                user
            };
            globals.user.replace(user);
            return Ok(())
        }

        if let Ok(req) = SendChat::try_from(frame.clone()) {
            let message = req.message;
            if let Some(user) = &globals.user {
                let user = User(user.clone());
                let chat = Chat(user, message);
                globals.bus.chat(chat)?;
            }
            return Ok(())
        }

        if let Ok(req) = SendInstantMessage::try_from(frame.clone()) {
            let SendInstantMessage { user_id, message } = req;
            let to = globals.user_find(user_id);
            let Globals { user, bus, .. } = globals;
            if let (Some(from), Some(to)) = (user, to) {
                bus.instant_message(
                    InstantMessage {
                        from: from.clone().into(),
                        to: to.into(),
                        message,
                    }
                )?;
            }
            let reply = SendInstantMessageReply.reply_to(&header);
            write_frame(w, reply).await?;
            return Ok(())
        }

        if let Ok(req) = SendBroadcast::try_from(frame.clone()) {
            let message = req.message;
            globals.bus.broadcast(message.into())?;
            let reply = SendBroadcastReply.reply_to(&header);
            write_frame(w, reply).await?;
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
            let user = globals.user_list()
                .into_iter()
                .find(|u| u.user_id == user_id);
            if let Some(user) = user {
                let text = format!("{:#?}", &user).replace("\n", "\r");
                let reply = GetClientInfoTextReply {
                    user_name: user.username,
                    text: text.into_bytes(),
                }.reply_to(&header);
                write_frame(w, reply).await?;
                return Ok(())
            }
        }

        eprintln!("established: unhandled request {:?} {:?}", header, body);

        Ok(())
    }
    fn disconnect(&mut self) {
        let Self { globals, .. } = self;
        let Globals { user, .. } = &globals;
        if let Some(user) = user.clone() {
            globals.user_remove(&user);
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
