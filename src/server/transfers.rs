use deku::prelude::*;
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
use tracing::{Instrument as _, debug, info_span, instrument};

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
    Error(proto::ErrorCode),
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
        id
    }
    fn add_upload(&mut self, conn: oneshot::Sender<TS>) -> ReferenceNumber {
        let id = self.next_id();
        self.uploads.insert(id, conn);
        debug!("added upload {id:?}, size={}", self.uploads.len());
        id
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

pub struct FlattenedFileStream<S> {
    stream: S,
    hdr: Option<proto::FlattenedFileHeader>,
    info: Option<proto::InfoFork>,
    position: u16,
}

impl<TS: TransferStream> FlattenedFileStream<TS> {
    fn new(stream: TS) -> Self {
        Self {
            stream,
            hdr: None,
            info: None,
            position: 0,
        }
    }
    async fn header(&mut self) -> TransferResult<proto::FlattenedFileHeader> {
        if let Some(hdr) = self.hdr {
            return Ok(hdr);
        }
        let hdr = self.read_file_header().await?;
        self.hdr.replace(hdr);
        Ok(hdr)
    }
    pub async fn info(&mut self) -> TransferResult<proto::InfoFork> {
        if let Some(info) = self.info.clone() {
            return Ok(info);
        }
        self.header().await?;
        let hdr = self.read_fork_header().await?;
        if hdr.fork_type != proto::ForkType::Info {
            return Err(proto::ProtocolError::ParseHeader.into());
        };
        let mut buf = [0u8; proto::InfoForkHeader::SIZE_BYTES.unwrap()];
        self.stream.read_exact(&mut buf).await?;
        let info_hdr = match proto::InfoForkHeader::try_from(&buf[..]) {
            Ok(hdr) => hdr,
            Err(e) => return Err(proto::ProtocolError::from(e).into()),
        };
        let filename = self.read_pstring().await?;
        let comment = self.read_pstring().await?;
        let info = proto::InfoFork {
            header: info_hdr,
            filename,
            comment,
        };
        self.info.replace(info.clone());
        self.position = 1;
        Ok(info)
    }
    async fn read_pstring(&mut self) -> TransferResult<proto::PString> {
        let len = self.stream.read_u16().await?;
        let mut val = vec![0u8; len as usize];
        if len > 0 {
            self.stream.read_exact(&mut val[..len as usize]).await?;
        }
        Ok(val.into())
    }
    pub async fn next(&mut self) -> TransferResult<Option<proto::ForkHeader>> {
        let hdr = self.header().await?;
        let _ = self.info().await?;

        let fork_count = u16::from(hdr.fork_count);

        if self.position >= fork_count {
            return Ok(None);
        }

        let fork_hdr = self.read_fork_header().await?;

        self.position += 1;

        Ok(Some(fork_hdr))
    }
    async fn read_file_header(&mut self) -> TransferResult<proto::FlattenedFileHeader> {
        let mut buf = [0u8; proto::FlattenedFileHeader::SIZE_BYTES.unwrap()];
        self.stream.read_exact(&mut buf).await?;
        match proto::FlattenedFileHeader::try_from(&buf[..]) {
            Ok(header) => Ok(header),
            Err(e) => Err(proto::ProtocolError::from(e).into()),
        }
    }
    async fn read_fork_header(&mut self) -> TransferResult<proto::ForkHeader> {
        let mut buf = [0u8; proto::ForkHeader::SIZE_BYTES.unwrap()];
        self.stream.read_exact(&mut buf).await?;
        match proto::ForkHeader::try_from(&buf[..]) {
            Ok(header) => Ok(header),
            Err(e) => Err(proto::ProtocolError::from(e).into()),
        }
    }
}

impl<TS: TransferStream> AsMut<TS> for FlattenedFileStream<TS> {
    fn as_mut(&mut self) -> &mut TS {
        &mut self.stream
    }
}

struct UploadTransfer<S> {
    stream: S,
    path: PathBuf,
    files: OsFiles,
}

impl<TS: TransferStream + 'static> UploadTransfer<TS> {
    async fn run(mut self) -> TransferResult<()> {
        let stream = FlattenedFileStream::new(&mut self.stream);
        self.files.write_adfs(&self.path, stream).await?;
        Ok(())
    }
}

impl<TS: TransferStream + 'static> TransfersUpdateProcessor<TS> {
    fn new(queue: mpsc::Receiver<Command<TS>>) -> Self {
        let requests = Requests::default();
        Self { queue, requests }
    }
    #[tracing::instrument(name = "TransfersUpdateProcessor", skip(self))]
    pub async fn run(mut self) {
        while let Some(command) = self.queue.recv().await {
            match command {
                Command::Transfer(Request::FileDownload { root, path }, tx) => {
                    let reply = match self.handle_download(&root, &path, 0).await {
                        Ok(reply) => reply.into(),
                        Err(e) => {
                            tracing::error!("error handling download request: {e:?}");
                            TransferReply::Error(0.into())
                        }
                    };
                    tx.send(reply).ok();
                }
                Command::Transfer(Request::FileUpload { root, path }, tx) => {
                    let reply = match self.handle_upload(&root, &path, 0).await {
                        Ok(reply) => reply.into(),
                        Err(e) => {
                            tracing::error!("error handling upload request: {e:?}");
                            TransferReply::Error(0.into())
                        }
                    };
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
