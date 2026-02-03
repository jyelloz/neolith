use anyhow::Result;
use futures::{SinkExt as _, StreamExt, channel::mpsc};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tower::Service;
use tracing::{error, info};

use crate::{
    protocol::{
        self as proto, ClientHandshakeRequest, HotlineProtocol, ProtocolVersion,
        ServerHandshakeReply, TransactionFrame,
    },
    server::{ClientRequest, ServerResponse, transaction_stream::Frames},
};

async fn handle_request(req: ClientRequest) -> anyhow::Result<Option<ServerResponse>> {
    info!("req {req:?}");
    let response = match req {
        ClientRequest::Login(..) => Some(ServerResponse::LoginReply),
        ClientRequest::GetMessages(..) => Some(ServerResponse::GetMessagesReply(
            proto::GetMessagesReply::single(b"News\r".to_vec().into()),
        )),
        // ClientRequest::PostNews(post_news) => todo!(),
        ClientRequest::GetFileNameList(..) => Some(ServerResponse::GetFileNameListReply(
            proto::GetFileNameListReply::single(proto::FileNameWithInfo {
                file_type: (*b"APPL").into(),
                creator: (*b"ttxt").into(),
                file_size: 134.into(),
                name_script: Default::default(),
                file_name_size: Default::default(),
                file_name: b"SimpleText".to_vec().into(),
            }),
        )),
        ClientRequest::GetFileInfo(..) => {
            Some(ServerResponse::GetFileInfoReply(proto::GetFileInfoReply {
                filename: b"SimpleText".to_vec().into(),
                size: 134.into(),
                type_code: (*b"APPL").into(),
                creator: b"ttxt".to_vec().into(),
                comment: b"comment".to_vec().into(),
                created_at: Default::default(),
                modified_at: Default::default(),
            }))
        }
        // ClientRequest::SetFileInfo(set_file_info) => todo!(),
        ClientRequest::GetUserNameList(..) => Some(ServerResponse::GetUserNameListReply(
            proto::GetUserNameListReply::single(proto::UserNameWithInfo {
                user_id: 1.into(),
                icon_id: 410.into(),
                user_flags: Default::default(),
                username_len: Default::default(),
                username: b"nobody".to_vec().into(),
            }),
        )),
        ClientRequest::GetClientInfoText(..) => Some(ServerResponse::GetClientInfoTextReply(
            proto::GetClientInfoTextReply {
                user_name: b"nobody".to_vec().into(),
                text: b"nothing".to_vec().into(),
            },
        )),
        // ClientRequest::SetClientUserInfo(set_client_user_info) => todo!(),
        // ClientRequest::DisconnectUser(disconnect_user) => todo!(),
        // ClientRequest::SendChat(send_chat) => todo!(),
        // ClientRequest::SendInstantMessage(send_instant_message) => todo!(),
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
        // ClientRequest::SendBroadcast(send_broadcast) => todo!(),
        _ => Some(ServerResponse::Rejected(Some("TODO".to_string()))),
    };
    Ok(response)
}

#[tracing::instrument(name = "conn", skip(r, w))]
pub async fn handle_conn<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    id: u32,
    mut r: R,
    mut w: W,
) -> Result<()> {
    let _ = handshake(&mut r, &mut w).await?;

    let (tx, rx) = mpsc::unbounded();

    let reader = read_loop(r, tx);
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
async fn read_loop<R: AsyncRead + Unpin>(
    r: R,
    mut tx: mpsc::UnboundedSender<TransactionFrame>,
) -> Result<()> {
    let mut frames = Box::pin(Frames::new(r).frames());

    let mut svc = tower::ServiceBuilder::new()
        .map_request(|frame: TransactionFrame| ClientRequest::try_from(frame).ok().unwrap())
        .layer_fn(|svc| svc)
        .service_fn(handle_request);

    while let Some(Ok(f)) = frames.next().await {
        let resp = svc.call(f.clone()).await?;
        let Some(resp) = resp else {
            continue;
        };
        tracing::info!("res {resp:?}");
        let reply = TransactionFrame::from(resp).reply_to(&f.header);
        if let Err(e) = tx.send(reply).await {
            error!(
                "failed to send response to transaction #{:?}: {e:?}",
                f.header.id
            );
        }
    }

    Ok(())
}

#[tracing::instrument(name = "tx", skip_all)]
async fn write_loop<W: AsyncWrite + Unpin>(
    mut w: W,
    mut rx: mpsc::UnboundedReceiver<TransactionFrame>,
) -> Result<()> {
    while let Some(resp) = rx.next().await {
        write_frame(&mut w, resp).await?;
    }
    Ok(())
}

async fn write_frame<W: AsyncWrite + Unpin, H: HotlineProtocol>(w: &mut W, h: H) -> Result<()> {
    w.write_all(&h.into_bytes()).await?;
    Ok(())
}
