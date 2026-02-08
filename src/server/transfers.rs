use deku::prelude::*;
use derive_more::{From, Into};
use std::{
    collections::HashMap,
    num::TryFromIntError,
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;
use tokio::{
    io::{self, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{Mutex, mpsc, oneshot},
};
use tracing::{Instrument as _, debug, error, info_span, instrument};

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

pub trait TransferStream: AsyncRead + AsyncWrite + Send + Unpin + std::fmt::Debug {}

impl<TS: AsyncRead + AsyncWrite + Send + Unpin + std::fmt::Debug> TransferStream for TS {}

pub struct RequestsInner<TS> {
    downloads: HashMap<ReferenceNumber, oneshot::Sender<TS>>,
    uploads: HashMap<ReferenceNumber, oneshot::Sender<TS>>,
    next_id: u32,
}

impl<TS> RequestsInner<TS> {
    fn next_id(&mut self) -> ReferenceNumber {
        let id = self.next_id.into();
        self.next_id += 1;
        id
    }
    fn get_download(&mut self, id: ReferenceNumber) -> Option<oneshot::Sender<TS>> {
        self.downloads.remove(&id)
    }
    fn get_upload(&mut self, id: ReferenceNumber) -> Option<oneshot::Sender<TS>> {
        self.uploads.remove(&id)
    }
    fn add_download(&mut self, conn: oneshot::Sender<TS>) -> ReferenceNumber {
        let id = self.next_id();
        self.downloads.insert(id, conn);
        debug!("added download {id:?}, size={}", self.uploads.len());
        id.into()
    }
    fn add_upload(&mut self, conn: oneshot::Sender<TS>) -> ReferenceNumber {
        let id = self.next_id();
        self.uploads.insert(id, conn);
        debug!("added upload {id:?}, size={}", self.uploads.len());
        id.into()
    }
}

impl<TS> Default for RequestsInner<TS> {
    fn default() -> Self {
        Self {
            downloads: Default::default(),
            uploads: Default::default(),
            next_id: 0,
        }
    }
}

pub struct Requests<TS> {
    inner: Arc<Mutex<RequestsInner<TS>>>,
}

impl<TS> Default for Requests<TS> {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Default::default())),
        }
    }
}

impl<TS> Requests<TS> {
    pub async fn get_download(&self, id: ReferenceNumber) -> Option<oneshot::Sender<TS>> {
        let mut inner = self.inner.lock().await;
        inner.get_download(id)
    }
    pub async fn get_upload(&self, id: ReferenceNumber) -> Option<oneshot::Sender<TS>> {
        let mut inner = self.inner.lock().await;
        inner.get_upload(id)
    }
    pub async fn add_download(&mut self, conn: oneshot::Sender<TS>) -> ReferenceNumber {
        let mut inner = self.inner.lock().await;
        inner.add_download(conn)
    }
    async fn add_upload(&mut self, conn: oneshot::Sender<TS>) -> ReferenceNumber {
        let mut inner = self.inner.lock().await;
        inner.add_upload(conn)
    }
}

pub struct TransferConnection<TS> {
    transfers: TransfersService<TS>,
    stream: TS,
}

impl<TS: TransferStream + 'static> TransferConnection<TS> {
    pub fn new(stream: TS, transfers: TransfersService<TS>) -> Self {
        Self { stream, transfers }
    }
    #[tracing::instrument(skip(self), fields(reference))]
    pub async fn run(mut self) -> TransferResult<()> {
        let handshake = self.read_handshake().await?;
        debug!("handshake={:?}", &handshake);
        tracing::Span::current().record(
            "reference",
            format!("{:#x}", u32::from(handshake.reference)),
        );
        let id = handshake.reference;
        if handshake.is_upload() {
            self.transfers.start_upload(id, self.stream).await;
        } else {
            self.transfers.start_download(id, self.stream).await;
        }
        Ok(())
    }
    async fn read_handshake(&mut self) -> TransferResult<proto::TransferHandshake> {
        let mut buf = Box::pin(vec![0u8; 16]);
        self.stream.read_exact(&mut buf).await?;
        let handshake = <proto::TransferHandshake as HotlineProtocol>::from_bytes(&buf[..])?;
        Ok(handshake)
    }
}

enum StartTransferReply {
    Success,
    Failure,
}

enum Command<S> {
    Transfer(Request, oneshot::Sender<TransferReply>),
    StartDownload(ReferenceNumber, S, oneshot::Sender<StartTransferReply>),
    StartUpload(ReferenceNumber, S, oneshot::Sender<StartTransferReply>),
}

#[derive(Debug)]
pub struct TransfersService<S> {
    _bus: Bus,
    tx: mpsc::Sender<Command<S>>,
}

impl<S> Clone for TransfersService<S> {
    fn clone(&self) -> Self {
        Self {
            _bus: self._bus.clone(),
            tx: self.tx.clone(),
        }
    }
}

impl<TS: TransferStream + 'static> TransfersService<TS> {
    pub fn new(bus: Bus) -> (Self, TransfersUpdateProcessor<TS>) {
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
    pub async fn start_download(&mut self, reference: proto::ReferenceNumber, conn: TS) {
        let Self { tx: sender, .. } = self;
        let (tx, rx) = oneshot::channel();
        let cmd = Command::StartDownload(reference, conn, tx);
        sender.send(cmd).await.ok();
        let _ = rx.await;
    }
    pub async fn start_upload(&mut self, reference: proto::ReferenceNumber, conn: TS) {
        let Self { tx: sender, .. } = self;
        let (tx, rx) = oneshot::channel();
        let cmd = Command::StartUpload(reference, conn, tx);
        sender.send(cmd).await.ok();
        let _ = rx.await;
    }
}

pub struct TransfersUpdateProcessor<S> {
    queue: mpsc::Receiver<Command<S>>,
    requests: Requests<S>,
}

struct DownloadTransfer<TS> {
    stream: TS,
    file: proto::FlattenedFileObject,
}

impl<TS: TransferStream + 'static> DownloadTransfer<TS> {
    async fn run(mut self) -> TransferResult<()> {
        debug!("beginning transfer");
        let (info_header, info) = self.file.info();
        let header = self.file.header();
        let header = header.to_bytes().unwrap();
        self.stream.write_all(&header).await?;
        let info_header = info_header.to_bytes().unwrap();
        self.stream.write_all(&info_header).await?;
        let info = info.to_bytes().unwrap();
        self.stream.write_all(&info).await?;
        if let Some((header, body)) = self.file.take_fork(proto::ForkType::Resource) {
            debug!("sending resource fork");
            let size = self.copy_fork(header, body).await?;
            tracing::Span::current().record("rsrc_size", size);
        }
        if let Some((header, body)) = self.file.take_fork(proto::ForkType::Data) {
            debug!("sending data fork");
            let size = self.copy_fork(header, body).await?;
            tracing::Span::current().record("data_size", size);
        }
        debug!("done");
        Ok(())
    }
    async fn copy_fork(
        &mut self,
        header: proto::ForkHeader,
        body: proto::AsyncDataSource,
    ) -> io::Result<u64> {
        let bytes = header.to_bytes().unwrap();
        self.stream.write_all(&bytes).await?;
        let (len, fork) = body.into();
        let mut fork = fork.take(len);
        let bytes = tokio::io::copy(&mut fork, &mut self.stream).await?;
        Ok(bytes)
    }
}

struct UploadTransfer<S> {
    stream: S,
    path: PathBuf,
    files: OsFiles,
}

impl<TS: TransferStream + 'static> UploadTransfer<TS> {
    async fn run(mut self) -> TransferResult<()> {
        let path = self.path.clone();
        let header = self.read_file_header().await?;
        debug!("got header {header:?}");
        let _finf_header = self.read_fork_header().await?;
        let finf = self.read_file_info().await?;
        debug!("got finf {finf:?}");
        for _ in 1..header.fork_count.into() {
            let fork_header = self.read_fork_header().await?;
            let size = u32::from(fork_header.data_size) as u64;
            match fork_header.fork_type {
                proto::ForkType::Data => {
                    debug!("data fork {size} => {path:?}");
                    let mut stream = self.stream.take(size);
                    let mut file = self.files.write(&path, 0).await?;
                    tokio::io::copy(&mut stream, &mut file).await?;
                    self.stream = stream.into_inner();
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
                    let mut stream = self.stream.take(size);
                    let mut file = self.files.write(&rsrc_path, 0).await?;
                    file.write_all(hdr.to_bytes().unwrap().as_slice()).await?;
                    file.write_all(finf.to_bytes().unwrap().as_slice()).await?;
                    file.write_all(comment).await?;
                    tokio::io::copy(&mut stream, &mut file).await?;
                    self.stream = stream.into_inner();
                    debug!("copied rsrc fork");
                }
                fork => {
                    error!("ignoring {fork:?} fork");
                    tokio::io::copy(&mut self.stream, &mut tokio::io::sink()).await?;
                }
            }
        }
        Ok(())
    }
    async fn read_file_header(&mut self) -> TransferResult<proto::FlattenedFileHeader> {
        let mut buf = [0u8; 24];
        self.stream.read_exact(&mut buf).await?;
        match proto::FlattenedFileHeader::try_from(&buf[..]) {
            Ok(header) => Ok(header),
            _ => Err(proto::ProtocolError::ParseHeader.into()),
        }
    }
    async fn read_fork_header(&mut self) -> TransferResult<proto::ForkHeader> {
        let mut buf = [0u8; 16];
        self.stream.read_exact(&mut buf).await?;
        match proto::ForkHeader::try_from(&buf[..]) {
            Ok(header) => Ok(header),
            _ => Err(proto::ProtocolError::ParseHeader.into()),
        }
    }
    async fn read_file_info(&mut self) -> TransferResult<proto::InfoFork> {
        let mut buf = vec![0u8; 72];
        self.stream.read_exact(&mut buf[..72]).await?;
        let filename_len = u16::from_be_bytes([buf[70], buf[71]]) as usize;
        let mut filename = vec![0u8; filename_len + 2];
        self.stream
            .read_exact(&mut filename[..filename_len + 2])
            .await?;
        buf.extend(&filename);
        let comment_len =
            u16::from_be_bytes([filename[filename_len], filename[filename_len + 1]]) as usize;
        if comment_len > 0 {
            let mut comment = vec![0u8; comment_len];
            self.stream.read_exact(&mut comment[..comment_len]).await?;
            buf.extend(&comment);
        }
        match proto::InfoFork::try_from(&buf[..]) {
            Ok(info) => Ok(info),
            _ => Err(proto::ProtocolError::ParseHeader.into()),
        }
    }
    fn get_appledouble(path: &Path) -> PathBuf {
        let basename = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("._{}", name))
            .expect("no filename");
        path.to_path_buf().with_file_name(basename)
    }
}

impl<TS: TransferStream + 'static> TransfersUpdateProcessor<TS> {
    fn new(queue: mpsc::Receiver<Command<TS>>) -> Self {
        let requests = Requests::default();
        Self { queue, requests }
    }
    #[tracing::instrument(name = "TransfersUpdateProcessor", skip(self))]
    pub async fn run(mut self) -> TransferResult<()> {
        while let Some(command) = self.queue.recv().await {
            match command {
                Command::Transfer(Request::FileDownload { root, path }, tx) => {
                    let reply = self.handle_download(&root, &path, 0).await?;
                    tx.send(reply.into()).ok();
                }
                Command::Transfer(Request::FileUpload { root, path }, tx) => {
                    let reply = self.handle_upload(&root, &path, 0).await?;
                    tx.send(reply.into()).ok();
                }
                Command::StartDownload(id, conn, tx) => {
                    let Some(transfer) = self.requests.get_download(id).await else {
                        let _ = tx.send(StartTransferReply::Failure);
                        continue;
                    };
                    if let Err(_conn) = transfer.send(conn) {
                        let _ = tx.send(StartTransferReply::Failure);
                        continue;
                    }
                    let _ = tx.send(StartTransferReply::Success);
                }
                Command::StartUpload(id, conn, tx) => {
                    let Some(transfer) = self.requests.get_upload(id).await else {
                        let _ = tx.send(StartTransferReply::Failure);
                        continue;
                    };
                    if let Err(_conn) = transfer.send(conn) {
                        let _ = tx.send(StartTransferReply::Failure);
                        continue;
                    }
                    let _ = tx.send(StartTransferReply::Success);
                }
            };
        }
        Ok(())
    }
    #[instrument(skip(self))]
    async fn handle_download(
        &mut self,
        root: &Path,
        path: &Path,
        offset: u64,
    ) -> TransferResult<proto::DownloadFileReply> {
        let files = OsFiles::with_root(root).await?;
        let file = files.read(path).await?;
        let file_size = file.fork_len(proto::ForkType::Data).unwrap_or(0)
            + file.fork_len(proto::ForkType::Resource).unwrap_or(0);
        let (_, info) = file.info();
        let transfer_size = info.size() as u64 + file_size as u64 - offset;
        let (tx, rx) = oneshot::channel();
        let reference = self.requests.add_download(tx).await;
        let reply = proto::DownloadFileReply {
            transfer_size: transfer_size.try_into()?,
            file_size: file_size.try_into()?,
            reference,
            waiting_count: None,
        };
        tokio::spawn(
            async move {
                debug!("waiting for incoming connection");
                let stream = rx.await.expect("failed to await stream");
                let transfer = DownloadTransfer { stream, file };
                transfer.run().await
            }
            .instrument(info_span!("download", reference = u64::from(reference))),
        );
        Ok(reply)
    }
    #[instrument(skip(self))]
    async fn handle_upload(
        &mut self,
        root: &Path,
        path: &Path,
        _offset: u64,
    ) -> TransferResult<proto::UploadFileReply> {
        let files = OsFiles::with_root(root).await?;
        let (tx, rx) = oneshot::channel();
        let reference = self.requests.add_upload(tx).await;
        let reply = proto::UploadFileReply { reference };
        let path = path.to_path_buf();
        tokio::spawn(
            async move {
                let stream = rx.await.expect("failed to await stream");
                let transfer = UploadTransfer {
                    stream,
                    path,
                    files,
                };
                transfer.run().await
            }
            .instrument(info_span!("upload", reference = u64::from(reference))),
        );
        Ok(reply)
    }
}
