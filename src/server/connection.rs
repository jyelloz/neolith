use std::{
    collections::HashMap,
    future,
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use anyhow::Result;
use futures::{
    SinkExt as _, StreamExt,
    channel::mpsc::{self, UnboundedSender},
    lock::Mutex,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tower::Service;
use tracing::{error, info};

use crate::{
    protocol::{
        self as proto, ClientHandshakeRequest, HotlineProtocol, ProtocolVersion,
        ServerHandshakeReply, TransactionFrame,
    },
    server::{ClientRequest, ServerResponse, chat, files::OsFiles, transaction_stream::Frames},
};

pub trait ConnRead: AsyncRead + Unpin + Send + 'static {}
pub trait ConnWrite: AsyncWrite + Unpin + Send + 'static {}

impl<R: AsyncRead + Unpin + Send + 'static> ConnRead for R {}
impl<R: AsyncWrite + Unpin + Send + 'static> ConnWrite for R {}

#[derive(Clone, Debug)]
struct Peer {
    id: u32,
    nick: proto::Nickname,
    icon_id: proto::IconId,
    flags: proto::UserFlags,
    tx: mpsc::UnboundedSender<TransactionFrame>,
}

impl Peer {
    fn new(id: u32, tx: mpsc::UnboundedSender<TransactionFrame>) -> Self {
        Self {
            id,
            tx,
            nick: proto::Nickname::new_empty(),
            icon_id: 0.into(),
            flags: Default::default(),
        }
    }
}

impl From<&Peer> for proto::UserId {
    fn from(value: &Peer) -> Self {
        (value.id as u16).into()
    }
}

impl From<&Peer> for proto::Nickname {
    fn from(value: &Peer) -> Self {
        value.nick.clone()
    }
}

impl From<&Peer> for proto::UserNameWithInfo {
    fn from(value: &Peer) -> Self {
        Self {
            user_id: value.into(),
            icon_id: value.icon_id,
            user_flags: value.flags,
            username_len: Default::default(),
            username: value.into(),
        }
    }
}

impl From<&Peer> for proto::NotifyUserChange {
    fn from(value: &Peer) -> Self {
        Self {
            user_id: value.into(),
            icon_id: value.icon_id,
            user_flags: value.flags,
            username: value.into(),
        }
    }
}

impl From<&Peer> for proto::NotifyUserDelete {
    fn from(value: &Peer) -> Self {
        Self {
            user_id: value.into(),
        }
    }
}

#[derive(Clone, Default)]
pub struct Peers {
    peers: Arc<Mutex<HashMap<u32, Peer>>>,
}

impl Peers {
    async fn add(&self, id: u32, peer: Peer) {
        let mut peers = self.peers.lock().await;
        let pc = peers.values().cloned().collect::<Vec<_>>();
        let change = proto::NotifyUserChange::from(&peer);
        peers.insert(id, peer);
        drop(peers);
        for mut p in pc {
            let _ = p.tx.send(change.clone().into()).await;
        }
    }
    async fn update(&self, id: u32) {
        let peers = self.peers.lock().await;
        let Some(peer) = peers.get(&id).cloned() else {
            return;
        };
        let change = proto::NotifyUserChange::from(&peer);
        let pc = peers.values().cloned().collect::<Vec<_>>();
        drop(peers);
        for mut p in pc {
            let _ = p.tx.send(change.clone().into()).await;
        }
    }
    async fn remove(&self, id: u32) {
        let mut peers = self.peers.lock().await;
        peers.remove(&id);
        let pc = peers.values().cloned().collect::<Vec<_>>();
        drop(peers);
        let del = proto::NotifyUserDelete::from(proto::UserId::from(id as u16));
        for mut p in pc {
            let _ = p.tx.send(del.clone().into()).await;
        }
    }
    async fn get(&self, id: u32) -> Option<Peer> {
        let peers = self.peers.lock().await;
        peers.get(&id).cloned()
    }
}

#[derive(Clone)]
struct HlConn {
    tx: UnboundedSender<TransactionFrame>,
    id: u32,
    peers: Peers,
}

impl Service<TransactionFrame> for HlConn {
    type Response = Option<TransactionFrame>;
    type Error = proto::ProtocolError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, frame: TransactionFrame) -> Self::Future {
        let hdr = frame.header;
        let req = match ClientRequest::try_from(frame) {
            Err(e) => return Box::pin(future::ready(Err(e))),
            Ok(req) => req,
        };
        let tx = self.tx.clone();
        let peers = self.peers.clone();
        let id = self.id;
        let fut = async move {
            match handle_request(tx, id, peers, req).await {
                Ok(Some(resp)) => Ok(Some(TransactionFrame::from(resp).reply_to(&hdr))),
                Ok(None) => Ok(None),
                Err(e) => Err(e),
            }
        };
        Box::pin(fut)
    }
}

async fn handle_request(
    tx: UnboundedSender<TransactionFrame>,
    id: u32,
    peers: Peers,
    req: ClientRequest,
) -> Result<Option<ServerResponse>, proto::ProtocolError> {
    info!("req {req:?}");
    let Ok(fs) = OsFiles::with_root("files").await else {
        return Err(proto::ProtocolError::SystemError);
    };
    let response = match req {
        ClientRequest::Login(req) => {
            let mut peer = Peer::new(id, tx.clone());
            if let Some((nick, icon)) = req.nickname.zip(req.icon_id) {
                peer.nick = nick;
                peer.icon_id = icon;
                peers.add(id, peer).await;
            }
            Some(ServerResponse::LoginReply)
        }
        ClientRequest::GetMessages(..) => Some(ServerResponse::GetMessagesReply(
            proto::GetMessagesReply::single(b"News\r".to_vec().into()),
        )),
        // ClientRequest::PostNews(post_news) => todo!(),
        ClientRequest::GetFileNameList(proto::GetFileNameList(path)) => {
            let path = PathBuf::from(path);
            let files = match fs.list(&path).await {
                Err(e) => {
                    error!("failed to list files: {e:?}");
                    return Err(proto::ProtocolError::SystemError);
                }
                Ok(files) => files,
            };
            let files = files
                .into_iter()
                .filter_map(|path| proto::FileNameWithInfo::try_from(path).ok())
                .collect::<Vec<_>>();

            Some(ServerResponse::GetFileNameListReply(
                proto::GetFileNameListReply::with_files(files),
            ))
        }
        ClientRequest::GetFileInfo(proto::GetFileInfo { filename, path }) => {
            let path = PathBuf::from(path).join(PathBuf::from(&filename));
            let Ok(info) = fs.get_info(&path).await else {
                return Err(proto::ProtocolError::SystemError);
            };
            let reply = proto::GetFileInfoReply {
                filename: filename,
                size: (info.total_size() as u32).into(),
                type_code: proto::FileType::from(*info.file_type.bytes()),
                creator: info.creator.bytes().to_vec().into(),
                comment: info.comment.into(),
                created_at: info.created_at.into(),
                modified_at: info.modified_at.into(),
            };
            Some(ServerResponse::GetFileInfoReply(reply))
        }
        // ClientRequest::SetFileInfo(set_file_info) => todo!(),
        ClientRequest::GetUserNameList(..) => {
            let peers = peers
                .peers
                .lock()
                .await
                .values()
                .map(proto::UserNameWithInfo::from)
                .collect();
            Some(ServerResponse::GetUserNameListReply(
                proto::GetUserNameListReply::with_users(peers),
            ))
        }
        ClientRequest::GetClientInfoText(req) => {
            info!("find user {:?}", req.user_id);
            let Some(peer) = peers.get(u16::from(req.user_id) as u32).await else {
                return Ok(Some(ServerResponse::Error(None)));
            };
            let text = format!("{peer:#?}");
            info!("sending back {peer:?}");
            Some(ServerResponse::GetClientInfoTextReply(
                proto::GetClientInfoTextReply {
                    user_name: peer.nick.clone(),
                    text: text.as_bytes().to_vec(),
                },
            ))
        }
        ClientRequest::SetClientUserInfo(req) => {
            {
                let username = &req.username;
                let icon_id = req.icon_id;
                let mut peers = peers.peers.lock().await;
                peers
                    .entry(id)
                    .and_modify(move |peer| {
                        peer.nick = username.clone();
                        peer.icon_id = icon_id;
                    })
                    .or_insert_with(|| {
                        let mut peer = Peer::new(id, tx.clone());
                        peer.nick = username.clone();
                        peer.icon_id = icon_id;
                        peer
                    });
            }
            peers.update(id).await;
            None
        }
        // ClientRequest::DisconnectUser(disconnect_user) => todo!(),
        ClientRequest::SendChat(req) => {
            let Some(peer) = peers.get(id).await else {
                return Ok(Some(ServerResponse::Error(None)));
            };
            let formatted_chat = chat::format_chat(&peer.nick, req.message.as_slice());
            let msg = proto::ChatMessage {
                chat_id: req.chat_id,
                message: formatted_chat.to_vec(),
            };
            let mut peers = peers.peers.lock().await;
            for (_, peer) in peers.iter_mut() {
                let _ = peer.tx.send(msg.clone().into()).await;
            }
            None
        }
        ClientRequest::SendInstantMessage(req) => {
            let Some(peer) = peers.get(id).await else {
                return Ok(Some(ServerResponse::Error(None)));
            };
            let Some(mut to) = peers.get(req.user_id.into()).await else {
                return Ok(Some(ServerResponse::Error(None)));
            };
            let msg = proto::ServerMessage {
                user_id: Some((&peer).into()),
                user_name: Some(peer.nick.clone()),
                message: req.message,
            };
            let _ = to.tx.send(msg.into()).await;
            Some(ServerResponse::SendInstantMessageReply)
        }
        // ClientRequest::InviteToNewChat(invite_to_new_chat) => todo!(),
        // ClientRequest::InviteToChat(invite_to_chat) => todo!(),
        // ClientRequest::JoinChat(join_chat) => todo!(),
        // ClientRequest::LeaveChat(leave_chat) => todo!(),
        // ClientRequest::RejectChatInfo(reject_chat_invite) => todo!(),
        // ClientRequest::SetChatSubject(set_chat_subject) => todo!(),
        // ClientRequest::DownloadFile(download_file) => todo!(),
        // ClientRequest::UploadFile(upload_file) => todo!(),
        // ClientRequest::DeleteFile(delete_file) => todo!(),
        // ClientRequest::MoveFile(move_file) => todo!(),
        // ClientRequest::NewFolder(new_folder) => todo!(),
        // ClientRequest::MakeFileAlias(make_file_alias) => todo!(),
        // ClientRequest::NewUser(new_user) => todo!(),
        // ClientRequest::DeleteUser(delete_user) => todo!(),
        // ClientRequest::GetUser(get_user) => todo!(),
        // ClientRequest::SetUser(set_user) => todo!(),
        // ClientRequest::UserAccess => todo!(),
        ClientRequest::SendBroadcast(req) => {
            let msg = proto::ServerMessage {
                message: req.message,
                user_id: None,
                user_name: None,
            };
            let mut peers = peers.peers.lock().await;
            for (_, peer) in peers.iter_mut() {
                let _ = peer.tx.send(msg.clone().into()).await;
            }
            Some(ServerResponse::SendBroadcastReply)
        }
        _ => Some(ServerResponse::Error(Some("TODO".to_string()))),
    };
    Ok(response)
}

#[tracing::instrument(name = "conn", skip(r, w, peers, tx, rx))]
pub async fn handle_conn<R: ConnRead, W: ConnWrite>(
    id: u32,
    peers: Peers,
    tx: mpsc::UnboundedSender<TransactionFrame>,
    rx: mpsc::UnboundedReceiver<TransactionFrame>,
    mut r: R,
    mut w: W,
) -> Result<()> {
    {
        let mut r = Box::pin(&mut r);
        let mut w = Box::pin(&mut w);
        let _ = handshake(&mut r, &mut w).await?;
    }

    let reader = async move {
        let res = read_loop(r, id, peers.clone(), tx).await;
        peers.remove(id).await;
        res
    };
    let writer = write_loop(w, rx);

    info!("running");
    futures::try_join!(reader, writer)?;
    info!("done");

    Ok(())
}

async fn handshake<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    mut r: R,
    mut w: W,
) -> Result<ProtocolVersion> {
    let mut buf = [0u8; 12];
    r.read_exact(&mut buf).await?;
    let version = {
        ClientHandshakeRequest::try_from(&buf[..])?;
        ProtocolVersion::from(123)
    };

    let reply = ServerHandshakeReply::ok();
    write_frame(&mut w, reply).await?;

    Ok(version)
}

#[tracing::instrument(name = "rx", skip_all)]
async fn read_loop<R: ConnRead>(
    r: R,
    id: u32,
    peers: Peers,
    mut tx: mpsc::UnboundedSender<TransactionFrame>,
) -> Result<()> {
    let mut frames = Box::pin(Frames::new(r).frames());

    let mut svc = tower::ServiceBuilder::new().service(HlConn {
        tx: tx.clone(),
        id,
        peers,
    });

    while let Some(Ok(f)) = frames.next().await {
        let resp = svc.call(f.clone()).await?;
        let Some(resp) = resp else {
            continue;
        };
        let reply = resp.reply_to(&f.header);
        if let Err(e) = tx.send(reply).await {
            error!(
                "failed to send response to transaction #{:?}: {e:?}",
                f.header.id
            );
        }
    }

    info!("done");

    Ok(())
}

#[tracing::instrument(name = "tx", skip_all)]
async fn write_loop<W: AsyncWrite + Unpin>(
    mut w: W,
    mut rx: mpsc::UnboundedReceiver<TransactionFrame>,
) -> Result<()> {
    let mut id = 1u32;
    while let Some(mut frame) = rx.next().await {
        if !frame.is_reply() {
            frame.header.id = id.into();
            id = id.wrapping_add(1);
        }
        info!("frame {frame:?}");
        write_frame(&mut w, frame).await?;
    }
    info!("done");
    Ok(())
}

async fn write_frame<W: AsyncWrite + Unpin, H: HotlineProtocol>(w: &mut W, h: H) -> Result<()> {
    w.write_all(&h.into_bytes()).await?;
    Ok(())
}
