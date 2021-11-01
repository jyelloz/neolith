use tokio::{
    io::{
        AsyncRead,
        AsyncWrite,
        AsyncReadExt as _,
        AsyncWriteExt as _,
    },
    net::TcpListener,
};

use anyhow::bail;

type Result<T> = anyhow::Result<T>;

use neolith::protocol::{
    self,
    HotlineProtocol as _,
    ClientHandshakeRequest,
    GetFileNameList,
    GetFileNameListReply,
    FileNameWithInfo,
    GetMessages,
    GetMessagesReply,
    GetUserNameList,
    GetUserNameListReply,
    LoginReply,
    LoginRequest,
    Message,
    Nickname,
    ProtocolVersion,
    ServerHandshakeReply,
    SetClientUserInfo,
    TransactionBody,
    TransactionHeader,
    UserNameWithInfo,
};

#[tokio::main]
async fn main() -> Result<()> {

    let listener = TcpListener::bind("127.0.0.1:5500").await?;

    loop {
        let (socket, addr) = listener.accept().await?;
        let mut conn = Connection::new(socket);
        tokio::spawn(async move {
            while let Ok(_) = conn.read_frame().await { }
            eprintln!("disconnect from {:?}", addr);
        });
    }

}

enum State<S> {
    New(New<S>),
    Unauthenticated(Unauthenticated<S>),
    Established(Established<S>),
    Borrowed,
}

impl <S: AsyncRead + AsyncWrite + Unpin> State<S> {
    async fn process(&mut self) -> Result<()> {
        *self = match std::mem::replace(self, Self::Borrowed) {
            Self::Borrowed => {
                unreachable!("process() may not be called while borrowed")
            },
            Self::New(mut state) => {
                let version = state.handshake().await?;
                eprintln!("protocol version {:?}", version);
                let New(conn) = state;
                Self::Unauthenticated(Unauthenticated(conn))
            },
            Self::Unauthenticated(mut state) => {
                state.login().await?;
                let Unauthenticated(conn) = state;
                Self::Established(Established::new(conn))
            },
            Self::Established(mut state) => {
                state.process_transaction().await?;
                Self::Established(state)
            },
        };
        Ok(())
    }
}

struct Connection<S> {
    state: State<S>,
}

impl <S: AsyncRead + AsyncWrite + Unpin> Connection<S> {
    fn new(socket: S) -> Self {
        Self {
            state: State::New(New(socket)),
        }
    }
    async fn read_frame(&mut self) -> Result<()> {
        self.state.process().await?;
        Ok(())
    }
}

struct New<S>(S);
impl <S: AsyncRead + AsyncWrite + Unpin> New<S> {
    fn handshake_sync(&mut self, buf: &[u8]) -> Result<ProtocolVersion> {
        match ClientHandshakeRequest::from_bytes(&buf) {
            Ok((_, r)) => {
                let ClientHandshakeRequest { version, sub_version, .. } = r;
                dbg!(version);
                dbg!(sub_version);
                Ok(123i16.into())
            },
            Err(e) => bail!("failed to parse handshake request: {:?}", e),
        }
    }
    pub async fn handshake(&mut self) -> Result<ProtocolVersion> {

        let mut buf = [0u8; 12];
        self.0.read_exact(&mut buf).await?;
        let version = self.handshake_sync(&buf)?;

        let reply = ServerHandshakeReply::ok();
        self.0.write_all(&reply.into_bytes()).await?;

        eprintln!("replied to handshake");

        Ok(version)
    }
}

struct Unauthenticated<S>(S);
impl <S: AsyncRead + AsyncWrite + Unpin> Unauthenticated<S> {
    pub async fn login(&mut self) -> Result<LoginRequest> {

        let Self(conn) = self;

        let header = process_header(conn).await?;
        let body = process_transaction_body(conn, header.body_len()).await?;
        let frame = protocol::TransactionFrame { header, body };
        let login = LoginRequest::try_from(frame)?;

        eprintln!("get user name list");
        let reply: protocol::TransactionFrame = LoginReply::default().into();
        let reply = reply.reply_to(&header).into_bytes();
        conn.write_all(&reply).await?;

        eprintln!("logged in as {:?}", &login);

        Ok(login)
    }
}

struct Established<S>{
    conn: S,
}

impl <S: AsyncRead + AsyncWrite + Unpin> Established<S> {

    pub fn new(conn: S) -> Self {
        Self { conn }
    }

    pub async fn process_transaction(&mut self) -> Result<()> {
        let Self {
            conn,
            ..
        } = self;
        let header = process_header(conn).await?;
        let body = process_transaction_body(conn, header.body_len()).await?;

        let frame = protocol::TransactionFrame {
            header: header.clone(),
            body: body.clone(),
        };

        if let Ok(_) = GetUserNameList::try_from(frame.clone()) {
            eprintln!("get user name list");
            let user = UserNameWithInfo {
                user_id: 1i16.into(),
                icon_id: 145i16.into(),
                user_flags: 0.into(),
                username: Nickname::new(b"unnamed".to_vec()),
            };
            let reply = GetUserNameListReply::single(user);
            let reply: protocol::TransactionFrame = reply.into();
            let reply = reply.reply_to(&header);
            let reply = reply.into_bytes();
            self.conn.write_all(&reply).await?;
            return Ok(())
        }

        if let Ok(_) = GetMessages::try_from(frame.clone()) {
            eprintln!("get messages");
            let reply = GetMessagesReply::single(
                Message::new(b"news".to_vec())
            );
            let reply: protocol::TransactionFrame = reply.into();
            let reply = reply.reply_to(&header);
            let reply = reply.into_bytes();
            self.conn.write_all(&reply).await?;
            return Ok(())
        }

        if let Ok(get) = GetFileNameList::try_from(frame.clone()) {
            eprintln!("get files: {:?}", &get);
            let reply = GetFileNameListReply::single(
                FileNameWithInfo {
                    file_type: b"APPL".into(),
                    creator: b"BOBO".into(),
                    name_script: 1.into(),
                    file_size: (1 << 20).into(),
                    file_name: "ClarisWorks".into(),
                },
            );
            let reply: protocol::TransactionFrame = reply.into();
            let reply = reply.reply_to(&header);
            let reply = reply.into_bytes();
            self.conn.write_all(&reply).await?;
            return Ok(())
        }

        if let Ok(req) = SetClientUserInfo::try_from(frame.clone()) {
            eprintln!("setting user info to {:?}", &req);
            return Ok(())
        }

        eprintln!("established: unhandled request {} {:?}", header.body_len(), body);

        Ok(())
    }
}

async fn process_header<S: AsyncRead + Unpin>(socket: &mut S) -> Result<TransactionHeader> {
    let mut buf = [0u8; 20];
    socket.read_exact(&mut buf).await?;
    match TransactionHeader::from_bytes(&buf) {
        Ok((_, header)) => Ok(header),
        Err(e) => bail!("failed to parse transaction header: {:?}", e),
    }
}

async fn process_transaction_body<S: AsyncRead + Unpin>(socket: &mut S, size: usize) -> Result<TransactionBody> {
    let mut buf = &mut vec![0u8; size][..size];
    socket.read_exact(&mut buf).await?;
    match TransactionBody::from_bytes(&buf) {
        Ok((_, body)) => Ok(body),
        Err(e) => bail!("failed to parse transaction body: {:?}", e),
    }
}
