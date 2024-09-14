use std::{
    io::{self, SeekFrom},
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio::{
    fs,
    io::{
        AsyncRead,
        AsyncWrite,
        AsyncReadExt,
        AsyncWriteExt,
        AsyncSeekExt,
    },
    sync::{oneshot, mpsc, watch},
};
use derive_more::{From, Into};
use thiserror::Error;
use tracing::{debug, error};
use deku::prelude::*;

use crate::protocol as proto;

#[derive(Debug, Error)]
pub enum TransferError {
    #[error("i/o error")]
    IO(#[from] std::io::Error),
    #[error("protocol error")]
    Protocol(#[from] proto::ProtocolError),
    #[error("invalid upload or download request id")]
    InvalidRequest,
}

type Result<T> = ::core::result::Result<T, TransferError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, From, Into, Hash)]
struct RequestId(i32);

impl From<proto::ReferenceNumber> for RequestId {
    fn from(reference: proto::ReferenceNumber) -> Self {
        Self(reference.into())
    }
}

impl From<RequestId> for proto::ReferenceNumber {
    fn from(val: RequestId) -> Self {
        let RequestId(value) = val;
        value.into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Request {
    FileDownload { root: PathBuf, path: PathBuf },
    FileUpload { root: PathBuf, path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum TransferReply {
    FileDownload(proto::DownloadFileReply),
    FileUpload(proto::UploadFileReply),
}

impl From<proto::DownloadFileReply> for TransferReply {
    fn from(value: proto::DownloadFileReply) -> Self {
        Self::FileDownload(value)
    }
}

impl From<proto::UploadFileReply> for TransferReply {
    fn from(value: proto::UploadFileReply) -> Self {
        Self::FileUpload(value)
    }
}

#[derive(Debug, Default, Clone)]
pub struct Requests {
    requests: HashMap<RequestId, Request>,
    next_id: i32,
}

impl Requests {
    fn new() -> Self {
        Self {
            requests: Default::default(),
            next_id: i32::MIN,
        }
    }
    fn add_download(&mut self, root: PathBuf, path: PathBuf) -> RequestId {
        let id = self.next_id();
        self.requests.insert(id, Request::FileDownload { root, path });
        tracing::debug!("added transfer {id:?}, size={}", self.requests.len());
        id
    }
    fn add_upload(&mut self, root: PathBuf, path: PathBuf) -> RequestId {
        let id = self.next_id();
        self.requests.insert(id, Request::FileUpload { root, path });
        id
    }
    fn get(&self, id: RequestId) -> Option<&Request> {
        self.requests.get(&id)
    }
    fn remove(&mut self, id: RequestId) {
        self.requests.remove(&id);
        tracing::warn!("removed transfer {id:?}, size={}", self.requests.len());
    }
    fn next_id(&mut self) -> RequestId {
        let id = self.next_id.into();
        self.next_id += 1;
        id
    }
}

#[derive(Debug, Clone)]
pub struct Files(pub PathBuf);

impl Files {
    pub fn with_root(root: &Path) -> Self {
        Self(root.to_owned())
    }
    fn child(&self, path: &Path) -> PathBuf {
        let Self(root) = self;
        root.clone()
            .join(path)
    }
    async fn read(&self, path: &Path, offset: u64) -> io::Result<Box<dyn AsyncRead + Unpin + Send>> {
        let path = self.child(path);
        let mut file = fs::File::open(path).await?;
        file.seek(SeekFrom::Start(offset)).await?;
        Ok(Box::new(file))
    }
    pub async fn info(&self, path: &Path) -> io::Result<(u64, proto::InfoFork)> {
        debug!("info on {path:?}, root={:?}", self.0);
        let path = self.child(path);
        let stat = fs::metadata(&path).await?;
        debug!("found stat {stat:?}");
        let file_name = path.file_name()
            .expect("no file name")
            .to_str()
            .expect("filename must be a str")
            .to_string()
            .into_bytes();
        debug!("found filename {file_name:?}");
        let len = stat.len();
        let info = proto::InfoFork {
            platform: proto::PlatformType::AppleMac,
            type_code: proto::FileType::try_from(&b"BINA"[..]).unwrap(),
            creator_code: proto::Creator::try_from(&b"dosa"[..]).unwrap(),
            flags: Default::default(),
            platform_flags: Default::default(),
            created_at: Default::default(),
            modified_at: Default::default(),
            name_script: Default::default(),
            name_len: file_name.len() as i16,
            file_name,
            comment_len: 0,
            comment: vec![],
        };
        Ok((len, info))
    }
    async fn write(&self, path: &Path, offset: u64) -> io::Result<Box<dyn AsyncWrite + Unpin + Send>> {
        let path = self.child(path);
        let file = if offset > 0 {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .open(path)
                .await?;
            file.seek(SeekFrom::Start(offset)).await?;
            file
        } else {
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .open(path)
                .await?
        };
        Ok(Box::new(file))
    }
}

pub struct TransferConnection<S> {
    files: Files,
    transfers: TransfersService,
    requests: watch::Receiver<Requests>,
    socket: S,
}

impl <S> TransferConnection<S> {
    fn get_request(&self, id: RequestId) -> Result<Request> {
        self.requests.borrow()
            .get(id)
            .cloned()
            .ok_or(TransferError::InvalidRequest)
    }
    fn get_file_download(&self, id: RequestId) -> Result<PathBuf> {
        match self.get_request(id)? {
            Request::FileDownload { path, .. } => Ok(path),
            _ => Err(TransferError::InvalidRequest),
        }
    }
    fn get_file_upload(&self, id: RequestId) -> Result<PathBuf> {
        match self.get_request(id)? {
            Request::FileUpload { path, .. } => Ok(path),
            _ => Err(TransferError::InvalidRequest),
        }
    }
}

impl <S: AsyncRead + AsyncWrite + Unpin + Send> TransferConnection<S> {
    pub fn new(
        socket: S,
        root: PathBuf,
        transfers: TransfersService,
        requests: watch::Receiver<Requests>,
    ) -> Self {
        let files = Files(root);
        Self {
            socket,
            files,
            transfers,
            requests,
        }
    }
    #[tracing::instrument(skip(self), fields(reference))]
    pub async fn run(mut self) -> Result<()> {
        let handshake = self.read_handshake().await?;
        let mut transfers = self.transfers.clone();
        debug!("handshake={:?}", &handshake);
        tracing::Span::current()
            .record("reference", format!("{:#x}", i32::from(handshake.reference)));
        let id = handshake.reference.into();
        let result = if handshake.is_upload() {
            self.handle_file_upload(id, handshake.size).await
        } else {
            self.handle_file_download(id).await
        };
        transfers.complete(handshake.reference).await?;
        match result {
            Ok(_) => debug!("successful transfer"),
            Err(e) => error!("unsuccessful transfer: {e:?}"),
        }
        Ok(())
    }
    async fn read_handshake(&mut self) -> Result<proto::TransferHandshake> {
        let mut buf = Box::pin(vec![0u8; 16]);
        self.socket.read_exact(&mut buf).await?;
        debug!("handshake bytes={buf:?}");
        match proto::TransferHandshake::try_from(&buf[..]) {
            Ok(handshake) => Ok(handshake),
            _ => Err(proto::ProtocolError::ParseHeader.into()),
        }
    }
    async fn write_fork(
        socket: &mut S,
        header: proto::ForkHeader,
        body: proto::AsyncDataSource,
    ) -> io::Result<u64> {
        let bytes = header.to_bytes().unwrap();
        socket.write_all(&bytes).await?;
        let (_, mut fork) = body.into();
        let bytes = tokio::io::copy(&mut fork, socket).await?;
        Ok(bytes)
    }
    async fn handle_file_download(self, id: RequestId) -> Result<()> {
        let path = self.get_file_download(id)?;
        debug!("path {path:?}");
        let Self { mut socket, files, .. } = self;
        let (len, info) = files.info(&path).await?;
        let file = files.read(&path, 0).await?;
        let data = (len, file).into();
        let mut file = proto::FlattenedFileObject::with_data(info, data);
        let (info_header, info) = file.info();
        let header = file.header();
        let header = header.to_bytes().unwrap();
        socket.write_all(&header).await?;
        let info_header = info_header.to_bytes().unwrap();
        socket.write_all(&info_header).await?;
        let info = info.to_bytes().unwrap();
        socket.write_all(&info).await?;
        if let Some((header, body)) = file.take_fork(proto::ForkType::Data) {
            let size = Self::write_fork(&mut socket, header, body).await?;
            tracing::Span::current().record("data_size", size);
        }
        if let Some((header, body)) = file.take_fork(proto::ForkType::Resource) {
            let size = Self::write_fork(&mut socket, header, body).await?;
            tracing::Span::current().record("rsrc_size", size);
        }
        debug!("done");
        Ok(())
    }
    async fn handle_file_upload(self, id: RequestId, size: proto::DataSize) -> Result<()> {
        let path = self.get_file_upload(id)?;
        let Self { socket, .. } = self;
        let mut socket = socket.take(i32::from(size) as u64);
        let mut file = self.files.write(&path, 0).await?;
        tokio::io::copy(&mut socket, &mut file).await?;
        Ok(())
    }
    async fn read_file_header(&mut self) -> Result<proto::FlattenedFileHeader> {
        let mut buf = [0u8; 24];
        self.socket.read_exact(&mut buf).await?;
        match proto::FlattenedFileHeader::try_from(&buf[..]) {
            Ok(header) => Ok(header),
            _ => Err(proto::ProtocolError::ParseHeader.into()),
        }
    }
    async fn read_fork_header(&mut self) -> Result<proto::ForkHeader> {
        let mut buf = [0u8; 16];
        self.socket.read_exact(&mut buf).await?;
        match proto::ForkHeader::try_from(&buf[..]) {
            Ok(header) => Ok(header),
            _ => Err(proto::ProtocolError::ParseHeader.into()),
        }
    }
    async fn read_file_info(&mut self) -> Result<proto::InfoFork> {
        let mut buf = vec![0u8; 72];
        self.socket.read_exact(&mut buf[..72]).await?;
        let filename_len = i16::from_be_bytes([buf[70], buf[71]]) as usize;
        let mut filename = vec![0u8; filename_len + 2];
        self.socket.read_exact(&mut filename[..filename_len + 2]).await?;
        buf.extend(&filename);
        let comment_len = i16::from_be_bytes([
            filename[filename_len],
            filename[filename_len + 1],
        ]) as usize;
        if comment_len > 0 {
            let mut comment = vec![0u8; comment_len];
            self.socket.read_exact(&mut comment[..comment_len]).await?;
            buf.extend(&comment);
        }
        match proto::InfoFork::try_from(&buf[..]) {
            Ok(info) => Ok(info),
            _ => Err(proto::ProtocolError::ParseHeader.into()),
        }
    }
}

enum Command {
    Transfer(Request, oneshot::Sender<TransferReply>),
    Complete(RequestId, oneshot::Sender<()>),
}

#[derive(Debug, Clone)]
pub struct TransfersService(mpsc::Sender<Command>);

impl TransfersService {
    pub fn new() -> (Self, TransfersUpdateProcessor) {
        let (tx, rx) = mpsc::channel(10);
        let service = Self(tx);
        let process = TransfersUpdateProcessor::new(rx);
        (service, process)
    }
    pub async fn file_download(&mut self, root: PathBuf, path: PathBuf) -> Option<proto::DownloadFileReply> {
        let Self(queue) = self;
        let (tx, rx) = oneshot::channel();
        let cmd = Command::Transfer(Request::FileDownload { root, path }, tx);
        queue.send(cmd).await.ok();
        if let Ok(TransferReply::FileDownload(reply)) = rx.await {
            Some(reply)
        } else {
            None
        }
    }
    pub async fn file_upload(&mut self, root: PathBuf, path: PathBuf) -> Option<proto::UploadFileReply> {
        let Self(queue) = self;
        let (tx, rx) = oneshot::channel();
        let cmd = Command::Transfer(Request::FileUpload { root, path }, tx);
        queue.send(cmd).await.ok();
        if let Ok(TransferReply::FileUpload(reply)) = rx.await {
            Some(reply)
        } else {
            None
        }
    }
    pub async fn complete(&mut self, reference: proto::ReferenceNumber) -> Result<()> {
        let Self(queue) = self;
        let (tx, rx) = oneshot::channel();
        let id = i32::from(reference);
        let cmd = Command::Complete(id.into(), tx);
        queue.send(cmd).await.ok();
        rx.await.ok();
        Ok(())
    }
}

pub struct TransfersUpdateProcessor {
    queue: mpsc::Receiver<Command>,
    requests: Requests,
    updates: watch::Sender<Requests>,
}

impl TransfersUpdateProcessor {
    fn new(queue: mpsc::Receiver<Command>) -> Self {
        let requests = Requests::new();
        let (updates, _) = watch::channel(requests.clone());
        Self { queue, requests, updates }
    }
    #[tracing::instrument(name = "TransfersUpdateProcessor", skip(self))]
    pub async fn run(self) -> Result<()> {
        let Self { mut queue, mut requests, updates } = self;
        while let Some(command) = queue.recv().await {
            match command {
                Command::Transfer(Request::FileDownload { root, path }, tx) => {
                    let reply = Self::handle_download(
                        &root,
                        &path,
                        0,
                        &mut requests,
                    ).await?;
                    tx.send(reply.into()).ok();
                },
                Command::Transfer(Request::FileUpload { root, path }, tx) => {
                    let reply = Self::handle_upload(
                        &root,
                        &path,
                        0,
                        &mut requests,
                    ).await?;
                    tx.send(reply.into()).ok();
                },
                Command::Complete(id, tx) => {
                    requests.remove(id);
                    tx.send(()).ok();
                }
            };
            updates.send(requests.clone()).ok();
        }
        Ok(())
    }
    async fn handle_download(root: &Path, path: &Path, offset: u64, requests: &mut Requests) -> Result<proto::DownloadFileReply> {
        let files = Files::with_root(root);
        let (len, info) = files.info(path).await?;
        let transfer_size = (info.size() as u64 + len - offset) as i32;
        let reference = requests.add_download(root.to_path_buf(), path.to_path_buf());
        let reply = proto::DownloadFileReply {
            transfer_size: transfer_size.into(),
            file_size: (len as i32).into(),
            reference: reference.into(),
            waiting_count: None,
        };
        Ok(reply)
    }
    async fn handle_upload(root: &Path, path: &Path, offset: u64, requests: &mut Requests) -> Result<proto::UploadFileReply> {
        let reference = requests.add_upload(root.to_path_buf(), path.to_path_buf());
        Ok(proto::UploadFileReply { reference: reference.into() })
    }
    pub fn subscribe(&self) -> watch::Receiver<Requests> {
        self.updates.subscribe()
    }
}
