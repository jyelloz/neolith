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
};

use thiserror::Error;

use derive_more::{From, Into};

use crate::protocol::{
    HotlineProtocol as _,
    ProtocolError,
    DataSize,
    InfoFork,
    ForkHeader,
    ForkType,
    FlattenedFileHeader,
    FlattenedFileObject,
    ReferenceNumber,
    TransferHandshake,
};

#[derive(Debug, Error)]
pub enum TransferError {
    #[error("i/o error")]
    IO(#[from] std::io::Error),
    #[error("protocol error")]
    Protocol(#[from] ProtocolError),
    #[error("invalid upload or download request id")]
    InvalidRequest,
}

type Result<T> = ::core::result::Result<T, TransferError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, From, Into, Hash)]
struct RequestId(i32);

impl From<ReferenceNumber> for RequestId {
    fn from(reference: ReferenceNumber) -> Self {
        Self(reference.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Request {
    FileDownload(PathBuf),
    FileUpload(PathBuf),
}

#[derive(Debug, Default)]
struct Requests {
    requests: HashMap<RequestId, Request>,
}

impl Requests {
    fn new() -> Self {
        Self {
            requests: Default::default(),
        }
    }
    fn get(&self, id: RequestId) -> Option<&Request> {
        self.requests.get(&id)
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
    async fn info(&self, path: &Path) -> io::Result<(u64, InfoFork)> {
        let path = self.child(path);
        let stat = fs::metadata(&path).await?;
        let file_name = path.file_name()
            .expect("no file name")
            .to_str()
            .expect("filename cannot be a str")
            .to_string()
            .into_bytes();
        let len = stat.len();
        let info = InfoFork {
            platform: (*b"AMAC").into(),
            type_code: b"EPSF".into(),
            creator_code: b"BOBO".into(),
            flags: Default::default(),
            platform_flags: Default::default(),
            created_at: Default::default(),
            modified_at: Default::default(),
            name_script: Default::default(),
            file_name,
            comment: None,
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
                .create(true)
                .open(path)
                .await?
        };
        Ok(Box::new(file))
    }
}

pub struct TransferConnection<S> {
    files: Files,
    requests: Requests,
    socket: S,
}

impl <S> TransferConnection<S> {
    fn get_request(&self, id: RequestId) -> Result<Request> {
        self.requests.get(id)
            .cloned()
            .ok_or(TransferError::InvalidRequest)
    }
    fn get_file_download(&self, id: RequestId) -> Result<PathBuf> {
        match self.get_request(id)? {
            Request::FileDownload(path) => Ok(path),
            _ => Err(TransferError::InvalidRequest),
        }
    }
    fn get_file_upload(&self, id: RequestId) -> Result<PathBuf> {
        match self.get_request(id)? {
            Request::FileUpload(path) => Ok(path),
            _ => Err(TransferError::InvalidRequest),
        }
    }
}

impl <S: AsyncRead + AsyncWrite + Unpin + Send> TransferConnection<S> {
    pub fn new(socket: S, root: PathBuf) -> Self {
        let files = Files(root);
        let requests = Requests::default();
        Self {
            socket,
            files,
            requests,
        }
    }
    pub async fn run(mut self) -> Result<()> {
        let handshake = self.read_handshake().await?;
        eprintln!("handshake={:?}", &handshake);
        let TransferHandshake { reference, size } = handshake;
        let id = reference.into();
        if let Some(size) = size {
            self.handle_file_upload(id, size).await?;
        } else {
            self.handle_file_download(id).await?;
        }
        Ok(())
    }
    async fn read_handshake(&mut self) -> Result<TransferHandshake> {
        let mut buf = Box::pin(vec![0u8; 16]);
        self.socket.read_exact(&mut buf).await?;
        match TransferHandshake::from_bytes(&buf) {
            Ok((_, handshake)) => Ok(handshake),
            _ => Err(ProtocolError::ParseHeader.into()),
        }
    }
    async fn handle_file_download(self, id: RequestId) -> Result<()> {
        let path = self.get_file_download(id)?;
        eprintln!("{:?}", &path);
        let Self { mut socket, files, .. } = self;
        let (len, info) = files.info(&path).await?;
        let file = files.read(&path, 0).await?;
        let data = (len, file).into();
        let mut file = FlattenedFileObject::with_data(info, data);
        let (info_header, info) = file.info();
        let header = file.header();
        let header = header.into_bytes();
        socket.write_all(&header).await?;
        let info_header = info_header.into_bytes();
        socket.write_all(&info_header).await?;
        let info = info.into_bytes();
        socket.write_all(&info).await?;
        if let Some((rsrc_header, rsrc)) = file.take_fork(ForkType::Resource) {
            socket.write_all(&rsrc_header.into_bytes()).await?;
            let (_, mut fork) = rsrc.into();
            match tokio::io::copy(&mut fork, &mut socket).await {
                Err(e) => eprintln!("failed to send file: {:?}", e),
                _ => {},
            }
        }
        if let Some((data_header, data)) = file.take_fork(ForkType::Data) {
            socket.write_all(&data_header.into_bytes()).await?;
            let (_, mut fork) = data.into();
            match tokio::io::copy(&mut fork, &mut socket).await {
                Err(e) => eprintln!("failed to send file: {:?}", e),
                _ => {},
            }
        }
        Ok(())
    }
    async fn handle_file_upload(self, id: RequestId, size: DataSize) -> Result<()> {
        let path = self.get_file_upload(id)?;
        let mut file = self.files.write(&path, 0).await?;
        let mut transfer = self.socket.take(i32::from(size) as u64);
        tokio::io::copy(&mut transfer, &mut file).await?;
        Ok(())
    }
    async fn read_file_header(&mut self) -> Result<FlattenedFileHeader> {
        let mut buf = [0u8; 24];
        self.socket.read_exact(&mut buf).await?;
        match FlattenedFileHeader::from_bytes(&buf) {
            Ok((_, header)) => Ok(header),
            _ => Err(ProtocolError::ParseHeader.into()),
        }
    }
    async fn read_fork_header(&mut self) -> Result<ForkHeader> {
        let mut buf = [0u8; 16];
        self.socket.read_exact(&mut buf).await?;
        match ForkHeader::from_bytes(&buf) {
            Ok((_, header)) => Ok(header),
            _ => Err(ProtocolError::ParseHeader.into()),
        }
    }
    async fn read_file_info(&mut self) -> Result<InfoFork> {
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
        match InfoFork::from_bytes(&buf) {
            Ok((_, info)) => Ok(info),
            _ => Err(ProtocolError::ParseHeader.into()),
        }
    }
}
