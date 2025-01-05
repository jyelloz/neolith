use deku::prelude::*;
use derive_more::{From, Into};
use std::{
    collections::HashMap,
    io::{self, SeekFrom},
    path::{Path, PathBuf},
};
use thiserror::Error;
use tokio::{
    fs,
    io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt},
    sync::{mpsc, oneshot, watch},
};
use tracing::{debug, error, warn};

use crate::apple;
use crate::protocol::{self as proto, HotlineProtocol, ReferenceNumber};
use crate::server::bus::Bus;

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

#[derive(Debug, Clone)]
pub struct Files(pub PathBuf);

impl Files {
    pub fn with_root(root: &Path) -> Self {
        Self(root.to_owned())
    }
    fn child(&self, path: &Path) -> PathBuf {
        let Self(root) = self;
        root.clone().join(path)
    }
    async fn read(&self, path: &Path, offset: u64) -> io::Result<fs::File> {
        let path = self.child(path);
        let mut file = fs::File::open(path).await?;
        file.seek(SeekFrom::Start(offset)).await?;
        Ok(file)
    }
    fn get_appledouble(path: &Path) -> PathBuf {
        let basename = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("._{}", name))
            .expect("no filename");
        path.to_path_buf().with_file_name(basename)
    }
    fn get_filename(path: &Path) -> Vec<u8> {
        path.file_name()
            .expect("no file name")
            .to_str()
            .expect("filename must be a str")
            .to_string()
            .into_bytes()
    }
    async fn get_appledouble_file(&self, path: &Path) -> io::Result<fs::File> {
        let ad_path = Self::get_appledouble(path);
        self.read(&ad_path, 0).await
    }
    async fn get_appledouble_info(&self, path: &Path) -> io::Result<(u64, proto::InfoFork)> {
        let mut file = self.get_appledouble_file(path).await?;
        let mut buf = [0u8; apple::AppleSingleHeaderStub::calculate_size()];
        file.read_exact(&mut buf).await?;
        let header = apple::AppleSingleHeaderStub::try_from(&buf[..])?;
        let mut entries = vec![];
        for _ in 0..header.n_descriptors {
            let mut buf = [0u8; apple::EntryDescriptor::calculate_size()];
            file.read_exact(&mut buf).await?;
            let entry = apple::EntryDescriptor::try_from(&buf[..])?;
            entries.push(entry);
        }

        let rsrc_len = entries
            .iter()
            .filter_map(|e| e.rsrc())
            .map(|e| e.length as u64)
            .sum::<u64>();

        // TODO: use the info entry and seek to position
        let mut buf = [0u8; apple::FinderInfo::calculate_size()];
        file.read_exact(&mut buf).await?;

        let info = apple::FinderInfo::try_from(&buf[..])?;
        let file_name = Self::get_filename(path);

        let fork = proto::InfoFork {
            platform: proto::PlatformType::AppleMac,
            type_code: proto::FileType(info.file_type.0 .0),
            creator_code: proto::Creator(info.creator.0 .0),
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
        Ok((rsrc_len, fork))
    }
    pub async fn info(&self, path: &Path) -> io::Result<(u64, proto::InfoFork)> {
        debug!("info on {path:?}, root={:?}", self.0);
        let fs_path = self.child(path);
        let stat = fs::metadata(&fs_path).await?;
        debug!("found stat {stat:?}");
        let file_name = fs_path
            .file_name()
            .expect("no file name")
            .to_str()
            .expect("filename must be a str")
            .to_string()
            .into_bytes();
        debug!("found filename {file_name:?}");
        let len = stat.len();
        let (len, info) = match self.get_appledouble_info(path).await {
            Ok((rsrc_len, info)) => {
                debug!("for appledouble'd file {path:?} have data length {len}, rsrc length {rsrc_len}");
                (len + rsrc_len, info)
            }
            Err(e) => {
                debug!("no appledouble info: {e:?}");
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
                (len, info)
            }
        };
        debug!("info: {info:?}");
        Ok((len, info))
    }
    pub async fn rsrc(&self, path: &Path) -> io::Result<Option<(u64, fs::File)>> {
        let Ok(mut file) = self.get_appledouble_file(path).await else {
            return Ok(None);
        };
        debug!("have appledouble file");
        let mut buf = [0u8; apple::AppleSingleHeaderStub::calculate_size()];
        file.read_exact(&mut buf).await?;
        let header = apple::AppleSingleHeaderStub::try_from(&buf[..])?;
        debug!("have appledouble stub {header:?}");
        let mut entries = vec![];
        for _ in 0..header.n_descriptors {
            let mut buf = [0u8; apple::EntryDescriptor::calculate_size()];
            file.read_exact(&mut buf).await?;
            let entry = apple::EntryDescriptor::try_from(&buf[..])?;
            entries.push(entry);
        }
        let Some(rsrc_entry) = entries.iter().filter_map(|e| e.rsrc()).next() else {
            return Ok(None);
        };
        debug!("have rsrc entry {rsrc_entry:?}");
        file.seek(SeekFrom::Start(rsrc_entry.offset as u64)).await?;
        Ok(Some((rsrc_entry.length as u64, file)))
    }
    async fn write(
        &self,
        path: &Path,
        offset: u64,
    ) -> io::Result<Box<dyn AsyncWrite + Unpin + Send>> {
        let path = self.child(path);
        let file = if offset > 0 {
            let mut file = fs::OpenOptions::new().write(true).open(path).await?;
            file.seek(SeekFrom::Start(offset)).await?;
            file
        } else {
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
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
        debug!("handshake bytes={buf:?}");
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
        debug!("path {path:?}");
        let Self {
            mut socket, files, ..
        } = self;
        let (len, info) = files.info(&path).await?;
        debug!("info {info:?}");
        let file: Box<dyn AsyncRead + Send + Unpin> = Box::new(files.read(&path, 0).await?);
        let data = (len, file).into();
        let rsrc = files.rsrc(&path).await?;
        let mut file = if let Some((len, rsrc)) = rsrc {
            debug!("data+rsrc");
            let rsrc: Box<dyn AsyncRead + Send + Unpin> = Box::new(rsrc);
            let rsrc = (len, rsrc).into();
            proto::FlattenedFileObject::with_forks(info, data, rsrc)
        } else {
            debug!("data only");
            proto::FlattenedFileObject::with_data(info, data)
        };
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
    bus: Bus,
    tx: mpsc::Sender<Command>,
}

impl TransfersService {
    pub fn new(bus: Bus) -> (Self, TransfersUpdateProcessor) {
        let (tx, rx) = mpsc::channel(10);
        let service = Self { bus, tx };
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
        let files = Files::with_root(root);
        let (len, info) = files.info(path).await?;
        let transfer_size = (info.size() as u64 + len - offset) as i32;
        let reference = requests.add_download(root.to_path_buf(), path.to_path_buf());
        let reply = proto::DownloadFileReply {
            transfer_size: transfer_size.into(),
            file_size: (len as i32).into(),
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
