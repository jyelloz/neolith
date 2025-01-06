use deku::prelude::*;
use derive_more::{From, Into};
use std::{
    collections::HashMap,
    num::TryFromIntError,
    path::{Path, PathBuf},
};
use thiserror::Error;
use tokio::{
    io::{self, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{mpsc, oneshot, watch},
};
use tracing::{debug, error, warn};

use crate::apple;
use crate::protocol::{self as proto, HotlineProtocol, ReferenceNumber};
use crate::server::{bus::Bus, files::OsFiles};

#[derive(Debug, Error)]
pub enum TransferError {
    #[error("i/o error")]
    IO(#[from] std::io::Error),
    #[error("protocol error")]
    Protocol(#[from] proto::ProtocolError),
    #[error("file size")]
    FileSize(#[from] TryFromIntError),
    #[error("invalid upload or download request id")]
    InvalidRequest,
}

type TransferResult<T> = Result<T, TransferError>;

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
    requests: HashMap<ReferenceNumber, Request>,
    next_id: u32,
}

impl Requests {
    fn new() -> Self {
        Self {
            requests: Default::default(),
            next_id: u32::MIN,
        }
    }
    fn add_download(&mut self, root: PathBuf, path: PathBuf) -> ReferenceNumber {
        let id = self.next_id();
        self.requests
            .insert(id, Request::FileDownload { root, path });
        debug!("added transfer {id:?}, size={}", self.requests.len());
        id
    }
    fn add_upload(&mut self, root: PathBuf, path: PathBuf) -> ReferenceNumber {
        let id = self.next_id();
        self.requests.insert(id, Request::FileUpload { root, path });
        id
    }
    fn get(&self, id: ReferenceNumber) -> Option<&Request> {
        self.requests.get(&id)
    }
    fn remove(&mut self, id: ReferenceNumber) {
        self.requests.remove(&id);
        warn!("removed transfer {id:?}, size={}", self.requests.len());
    }
    fn next_id(&mut self) -> ReferenceNumber {
        let id = self.next_id.into();
        self.next_id += 1;
        id
    }
}

pub struct TransferConnection<S> {
    files: OsFiles,
    transfers: TransfersService,
    requests: watch::Receiver<Requests>,
    socket: S,
}

impl<S> TransferConnection<S> {
    fn get_request(&self, id: ReferenceNumber) -> TransferResult<Request> {
        self.requests
            .borrow()
            .get(id)
            .cloned()
            .ok_or(TransferError::InvalidRequest)
    }
    fn get_file_download(&self, id: ReferenceNumber) -> TransferResult<PathBuf> {
        match self.get_request(id)? {
            Request::FileDownload { path, .. } => Ok(path),
            _ => Err(TransferError::InvalidRequest),
        }
    }
    fn get_file_upload(&self, id: ReferenceNumber) -> TransferResult<PathBuf> {
        match self.get_request(id)? {
            Request::FileUpload { path, .. } => Ok(path),
            _ => Err(TransferError::InvalidRequest),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin + Send> TransferConnection<S> {
    pub fn new(
        socket: S,
        files: OsFiles,
        transfers: TransfersService,
        requests: watch::Receiver<Requests>,
    ) -> Self {
        Self {
            socket,
            files,
            transfers,
            requests,
        }
    }
    #[tracing::instrument(skip(self), fields(reference))]
    pub async fn run(mut self) -> TransferResult<()> {
        let handshake = self.read_handshake().await?;
        let mut transfers = self.transfers.clone();
        debug!("handshake={:?}", &handshake);
        tracing::Span::current().record(
            "reference",
            format!("{:#x}", u32::from(handshake.reference)),
        );
        let id = handshake.reference;
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
    async fn read_handshake(&mut self) -> TransferResult<proto::TransferHandshake> {
        let mut buf = Box::pin(vec![0u8; 16]);
        self.socket.read_exact(&mut buf).await?;
        let handshake = <proto::TransferHandshake as HotlineProtocol>::from_bytes(&buf[..])?;
        Ok(handshake)
    }
    async fn write_fork(
        socket: &mut S,
        header: proto::ForkHeader,
        body: proto::AsyncDataSource,
    ) -> io::Result<u64> {
        let bytes = header.to_bytes().unwrap();
        socket.write_all(&bytes).await?;
        let (len, fork) = body.into();
        let mut fork = fork.take(len);
        let bytes = tokio::io::copy(&mut fork, socket).await?;
        Ok(bytes)
    }
    fn get_appledouble(path: &Path) -> PathBuf {
        let basename = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("._{}", name))
            .expect("no filename");
        path.to_path_buf().with_file_name(basename)
    }
    async fn handle_file_download(self, id: ReferenceNumber) -> TransferResult<()> {
        let path = self.get_file_download(id)?;
        let Self {
            mut socket,
            files,
            ..
        } = self;
        let mut file = files.read(&path).await?;
        let (info_header, info) = file.info();
        let header = file.header();
        let header = header.to_bytes().unwrap();
        socket.write_all(&header).await?;
        let info_header = info_header.to_bytes().unwrap();
        socket.write_all(&info_header).await?;
        let info = info.to_bytes().unwrap();
        socket.write_all(&info).await?;
        if let Some((header, body)) = file.take_fork(proto::ForkType::Resource) {
            let size = Self::write_fork(&mut socket, header, body).await?;
            tracing::Span::current().record("rsrc_size", size);
        }
        if let Some((header, body)) = file.take_fork(proto::ForkType::Data) {
            let size = Self::write_fork(&mut socket, header, body).await?;
            tracing::Span::current().record("data_size", size);
        }
        debug!("done");
        Ok(())
    }
    async fn handle_file_upload(
        mut self,
        id: ReferenceNumber,
        _: proto::DataSize,
    ) -> TransferResult<()> {
        let path = self.get_file_upload(id)?;
        let header = self.read_file_header().await?;
        debug!("got header {header:?}");
        let _finf_header = self.read_fork_header().await?;
        let finf = self.read_file_info().await?;
        debug!("got finf {finf:?}");
        for _ in 1..header.fork_count.into() {
            let fork_header = self.read_fork_header().await?;
            let size = i32::from(fork_header.data_size) as u64;
            match fork_header.fork_type {
                proto::ForkType::Data => {
                    debug!("data fork {size} => {path:?}");
                    let mut socket = self.socket.take(size);
                    let mut file = self.files.write(&path, 0).await?;
                    tokio::io::copy(&mut socket, &mut file).await?;
                    self.socket = socket.into_inner();
                    debug!("copied data fork");
                }
                proto::ForkType::Resource => {
                    let finf_descriptor = apple::EntryDescriptor {
                        id: apple::EntryId::FinderInfo.into(),
                        length: apple::FinderInfo::calculate_size() as u32,
                        offset: 0,
                    };
                    let comment_descriptor = apple::EntryDescriptor {
                        id: apple::EntryId::Comment.into(),
                        length: finf.comment_len as u32,
                        offset: 0,
                    };
                    let rsrc_descriptor = apple::EntryDescriptor {
                        id: apple::EntryId::ResourceFork.into(),
                        length: size as u32,
                        offset: 0,
                    };
                    let entries = vec![finf_descriptor, comment_descriptor, rsrc_descriptor];
                    let hdr = apple::AppleSingleHeader::new_double(entries);

                    let flags_bytes: u32 = finf.platform_flags.into();
                    let flags = apple::FinderFlags::from(flags_bytes as u16);
                    let comment = finf.comment.as_slice();

                    let finf = apple::FinderInfo {
                        file_type: apple::FileType(finf.type_code.0.into()),
                        creator: apple::Creator(finf.creator_code.0.into()),
                        flags,
                        location: Default::default(),
                        folder: Default::default(),
                    };

                    let rsrc_path = Self::get_appledouble(&path);
                    debug!("rsrc fork {size} => {rsrc_path:?}");
                    let mut socket = self.socket.take(size);
                    let mut file = self.files.write(&rsrc_path, 0).await?;
                    file.write_all(hdr.to_bytes().unwrap().as_slice()).await?;
                    file.write_all(finf.to_bytes().unwrap().as_slice()).await?;
                    file.write_all(comment).await?;
                    tokio::io::copy(&mut socket, &mut file).await?;
                    self.socket = socket.into_inner();
                    debug!("copied rsrc fork");
                }
                fork => {
                    error!("ignoring {fork:?} fork");
                    tokio::io::copy(&mut self.socket, &mut tokio::io::sink()).await?;
                }
            }
        }

        debug!("done");

        Ok(())
    }
    async fn read_file_header(&mut self) -> TransferResult<proto::FlattenedFileHeader> {
        let mut buf = [0u8; 24];
        self.socket.read_exact(&mut buf).await?;
        match proto::FlattenedFileHeader::try_from(&buf[..]) {
            Ok(header) => Ok(header),
            _ => Err(proto::ProtocolError::ParseHeader.into()),
        }
    }
    async fn read_fork_header(&mut self) -> TransferResult<proto::ForkHeader> {
        let mut buf = [0u8; 16];
        self.socket.read_exact(&mut buf).await?;
        match proto::ForkHeader::try_from(&buf[..]) {
            Ok(header) => Ok(header),
            _ => Err(proto::ProtocolError::ParseHeader.into()),
        }
    }
    async fn read_file_info(&mut self) -> TransferResult<proto::InfoFork> {
        let mut buf = vec![0u8; 72];
        self.socket.read_exact(&mut buf[..72]).await?;
        let filename_len = i16::from_be_bytes([buf[70], buf[71]]) as usize;
        let mut filename = vec![0u8; filename_len + 2];
        self.socket
            .read_exact(&mut filename[..filename_len + 2])
            .await?;
        buf.extend(&filename);
        let comment_len =
            i16::from_be_bytes([filename[filename_len], filename[filename_len + 1]]) as usize;
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
    Complete(ReferenceNumber, oneshot::Sender<()>),
}

#[derive(Debug, Clone)]
pub struct TransfersService {
    _bus: Bus,
    tx: mpsc::Sender<Command>,
}

impl TransfersService {
    pub fn new(bus: Bus) -> (Self, TransfersUpdateProcessor) {
        let (tx, rx) = mpsc::channel(10);
        let service = Self { _bus: bus, tx };
        let process = TransfersUpdateProcessor::new(rx);
        (service, process)
    }
    pub async fn file_download(
        &mut self,
        root: PathBuf,
        path: PathBuf,
    ) -> Option<proto::DownloadFileReply> {
        let Self { tx: queue, .. } = self;
        let (tx, rx) = oneshot::channel();
        let cmd = Command::Transfer(Request::FileDownload { root, path }, tx);
        queue.send(cmd).await.ok();
        if let Ok(TransferReply::FileDownload(reply)) = rx.await {
            Some(reply)
        } else {
            None
        }
    }
    pub async fn file_upload(
        &mut self,
        root: PathBuf,
        path: PathBuf,
    ) -> Option<proto::UploadFileReply> {
        let Self { tx: queue, .. } = self;
        let (tx, rx) = oneshot::channel();
        let cmd = Command::Transfer(Request::FileUpload { root, path }, tx);
        queue.send(cmd).await.ok();
        if let Ok(TransferReply::FileUpload(reply)) = rx.await {
            Some(reply)
        } else {
            None
        }
    }
    pub async fn complete(&mut self, reference: proto::ReferenceNumber) -> TransferResult<()> {
        let Self { tx: queue, .. } = self;
        let (tx, rx) = oneshot::channel();
        let cmd = Command::Complete(reference, tx);
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
        Self {
            queue,
            requests,
            updates,
        }
    }
    #[tracing::instrument(name = "TransfersUpdateProcessor", skip(self))]
    pub async fn run(self) -> TransferResult<()> {
        let Self {
            mut queue,
            mut requests,
            updates,
        } = self;
        while let Some(command) = queue.recv().await {
            match command {
                Command::Transfer(Request::FileDownload { root, path }, tx) => {
                    let reply = Self::handle_download(&root, &path, 0, &mut requests).await?;
                    tx.send(reply.into()).ok();
                }
                Command::Transfer(Request::FileUpload { root, path }, tx) => {
                    let reply = Self::handle_upload(&root, &path, 0, &mut requests).await?;
                    tx.send(reply.into()).ok();
                }
                Command::Complete(id, tx) => {
                    requests.remove(id);
                    tx.send(()).ok();
                }
            };
            updates.send(requests.clone()).ok();
        }
        Ok(())
    }
    async fn handle_download(
        root: &Path,
        path: &Path,
        offset: u64,
        requests: &mut Requests,
    ) -> TransferResult<proto::DownloadFileReply> {
        let files = OsFiles::with_root(root).await?;
        let file = files.read(path).await?;
        let file_size = file.fork_len(proto::ForkType::Data).unwrap_or(0)
            + file.fork_len(proto::ForkType::Resource).unwrap_or(0);
        let (_, info) = file.info();
        let transfer_size = info.size() as u64 + file_size as u64 - offset;
        let reference = requests.add_download(root.to_path_buf(), path.to_path_buf());
        let reply = proto::DownloadFileReply {
            transfer_size: transfer_size.try_into()?,
            file_size: file_size.try_into()?,
            reference,
            waiting_count: None,
        };
        Ok(reply)
    }
    async fn handle_upload(
        root: &Path,
        path: &Path,
        _offset: u64,
        requests: &mut Requests,
    ) -> TransferResult<proto::UploadFileReply> {
        let reference = requests.add_upload(root.to_path_buf(), path.to_path_buf());
        Ok(proto::UploadFileReply { reference })
    }
    pub fn subscribe(&self) -> watch::Receiver<Requests> {
        self.updates.subscribe()
    }
}
