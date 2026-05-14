use crate::{
    protocol::{self as proto, AsyncDataSource, FlattenedFileObject},
    server::transfers::{FlattenedFileStream, TransferStream},
};
use adfs::asyncio as adfs;
use async_compat::Compat;
use deku::DekuContainerWrite as _;
use encoding_rs::MACINTOSH;
use four_cc::FourCC;
use futures_lite::{AsyncReadExt as _, AsyncSeekExt as _};
use magic::Cookie;
use std::{
    cell::RefCell,
    ffi::OsStr,
    fs::Metadata,
    io::{self, ErrorKind, SeekFrom},
    os::unix::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};
use tokio::fs;

#[derive(Debug)]
pub struct FileType(FourCC);

impl FileType {
    pub fn alias() -> Self {
        Self(b"alis".into())
    }
    pub fn directory() -> Self {
        Self(b"fldr".into())
    }
    pub fn bytes(&self) -> &[u8; 4] {
        &self.0.0
    }
}

impl Default for FileType {
    fn default() -> Self {
        Self(b"TEXT".into())
    }
}

#[derive(Debug)]
pub struct Creator(four_cc::FourCC);

impl Creator {
    pub fn of_alias() -> Self {
        Self::default()
    }
    pub fn of_directory() -> Self {
        Self::default()
    }
    pub fn bytes(&self) -> &[u8; 4] {
        &self.0.0
    }
}

impl Default for Creator {
    fn default() -> Self {
        Self(b"\0\0\0\0".into())
    }
}

pub struct DirEntry {
    pub path: PathBuf,
    pub data_len: u64,
    pub rsrc_len: u64,
    pub type_code: FileType,
    pub creator_code: Creator,
}

impl DirEntry {
    pub fn total_size(&self) -> u64 {
        self.data_len + self.rsrc_len
    }
}

impl TryFrom<DirEntry> for proto::FileNameWithInfo {
    type Error = io::Error;
    fn try_from(value: DirEntry) -> io::Result<Self> {
        let file_size = value
            .total_size()
            .try_into()
            .ok()
            .ok_or::<Self::Error>(io::ErrorKind::FileTooLarge.into())?;
        let DirEntry {
            creator_code,
            type_code,
            path,
            ..
        } = value;
        let file_name = path
            .file_name()
            .and_then(OsStr::to_str)
            .and_then(|s| {
                let (mac, _, errors) = MACINTOSH.encode(s);
                if errors { None } else { Some(mac.to_vec()) }
            })
            .ok_or::<Self::Error>(ErrorKind::InvalidData.into())?;
        let file_name_size = file_name.len() as u16;
        Ok(proto::FileNameWithInfo {
            file_name_size,
            file_name,
            file_size,
            creator: (*creator_code.bytes()).into(),
            file_type: (*type_code.bytes()).into(),
            name_script: 0.into(),
        })
    }
}

#[derive(Debug)]
pub struct FileInfo {
    pub path: PathBuf,
    pub data_len: u64,
    pub rsrc_len: u64,
    pub file_type: FileType,
    pub creator: Creator,
    pub comment: Vec<u8>,
    pub created_at: SystemTime,
    pub modified_at: SystemTime,
}

impl FileInfo {
    pub fn total_size(&self) -> u64 {
        self.data_len + self.rsrc_len
    }
}

impl TryFrom<(PathBuf, Metadata, ExtendedMetadata)> for FileInfo {
    type Error = std::io::Error;
    fn try_from(
        (path, metadata, magic): (PathBuf, Metadata, ExtendedMetadata),
    ) -> io::Result<Self> {
        let modified_at = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let created_at = metadata.created().unwrap_or(modified_at);
        let ExtendedMetadata {
            data_len,
            rsrc_len,
            file_type,
            creator,
            comment,
        } = magic;
        Ok(Self {
            data_len,
            rsrc_len,
            path,
            modified_at,
            created_at,
            file_type,
            creator,
            comment,
        })
    }
}

impl From<FileInfo> for proto::GetFileInfoReply {
    fn from(info: FileInfo) -> Self {
        let basename = info
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.as_bytes().to_vec())
            .unwrap_or_default();
        Self {
            filename: basename.into(),
            size: (info.total_size() as u32).into(),
            type_code: proto::FileType::from(*info.file_type.bytes()),
            creator: info.creator.bytes().to_vec().into(),
            comment: info.comment.into(),
            created_at: info.created_at.into(),
            modified_at: info.modified_at.into(),
        }
    }
}

#[derive(Debug)]
struct ExtendedMetadata {
    data_len: u64,
    rsrc_len: u64,
    file_type: FileType,
    creator: Creator,
    comment: Vec<u8>,
}

impl ExtendedMetadata {
    pub fn directory() -> Self {
        Self {
            data_len: 0,
            rsrc_len: 0,
            file_type: FileType::directory(),
            creator: Creator::of_directory(),
            comment: vec![],
        }
    }
}

thread_local! {
    static MAGIC: RefCell<Cookie<magic::cookie::Load>> = Cookie::open(magic::cookie::Flags::APPLE)
        .or::<io::Error>(Err(ErrorKind::Other.into()))
        .unwrap()
        .load(&Default::default())
        .or::<io::Error>(Err(ErrorKind::Other.into()))
        .map(RefCell::new)
        .unwrap();
}

#[derive(Debug, Clone)]
pub struct OsFiles {
    root: PathBuf,
}

impl OsFiles {
    pub async fn with_root<P: Into<PathBuf>>(root: P) -> io::Result<Self> {
        let root = root.into().canonicalize()?;
        let metadata = fs::metadata(&root).await?;
        if metadata.is_dir() {
            Ok(Self { root })
        } else {
            Err(ErrorKind::InvalidInput.into())
        }
    }
    pub async fn list(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        let path = self.subpath(path)?;
        let mut listing = adfs::read_dir(&path).await?;
        let mut entries = vec![];
        while let Some(ent) = futures::StreamExt::next(&mut listing).await {
            let Ok(ent) = ent else {
                continue;
            };
            let path = ent.inner.path();
            let metadata = self.resolve_metadata(&ent).await?;
            let dirent = self.decorate_metadata(path, &metadata).await?;
            entries.push(dirent);
        }
        Ok(entries)
    }
    async fn resolve_metadata(
        &self,
        dirent: &adfs::AppleDoubleDirEntry,
    ) -> io::Result<adfs::Metadata> {
        let mut metadata = dirent.metadata().await?;
        if !metadata.inner.is_symlink() {
            return Ok(metadata);
        }
        let resolved = fs::metadata(dirent.inner.path()).await?;
        metadata.inner = resolved;
        Ok(metadata)
    }
    async fn decorate_metadata(
        &self,
        path: PathBuf,
        metadata: &adfs::Metadata,
    ) -> io::Result<DirEntry> {
        let ExtendedMetadata {
            data_len,
            rsrc_len,
            file_type: type_code,
            creator: creator_code,
            ..
        } = if metadata.inner.is_dir() {
            ExtendedMetadata::directory()
        } else {
            self.appledouble_magic(metadata)
                .or_else(|_| self.apple_magic(&path, &metadata.inner))?
        };
        Ok(DirEntry {
            path,
            data_len,
            rsrc_len,
            type_code,
            creator_code,
        })
    }
    pub async fn get_info(&self, path: &Path) -> io::Result<FileInfo> {
        let path = self.subpath(path)?;
        let mut metadata = adfs::metadata(&path).await?;
        let info = if metadata.inner.is_dir() {
            ExtendedMetadata::directory()
        } else {
            self.appledouble_magic(&mut metadata)
                .or_else(|_| self.apple_magic(&path, &metadata.inner))?
        };
        (path, metadata.inner, info).try_into()
    }
    fn validate_path(path: &Path) -> io::Result<&Path> {
        let complex = path.components().any(|p| p == Component::ParentDir);
        if complex {
            return Err(ErrorKind::InvalidInput.into());
        }
        Ok(path)
    }
    fn subpath(&self, path: &Path) -> io::Result<PathBuf> {
        let Self { root, .. } = self;
        let path = Self::validate_path(path)?;
        let subpath = root.components().chain(path.components()).collect();
        Ok(subpath)
    }
    fn appledouble_file(data_path: &Path) -> adfs::AppleDoubleFile {
        adfs::AppleDoubleFile {
            data_path: data_path.to_path_buf(),
            metadata_path: Self::appledouble_path(data_path),
        }
    }
    fn appledouble_path(path: &Path) -> PathBuf {
        let basename = path.file_name().and_then(|p| p.to_str()).unwrap();
        let appledouble_basename = format!("._{basename}");
        Path::join(path.parent().unwrap(), appledouble_basename)
    }
    fn appledouble_magic(&self, metadata: &adfs::Metadata) -> io::Result<ExtendedMetadata> {
        let (ftyp, crea) = if let Some(finf) = metadata.finder_info.clone() {
            (finf.info.file_type.0, finf.info.creator.0)
        } else {
            (*b"BINA", *b"dosa")
        };
        let comment = metadata.comment.clone().unwrap_or_default();
        let info = ExtendedMetadata {
            data_len: metadata.inner.len(),
            rsrc_len: metadata.rsrc_size,
            file_type: FileType(FourCC(ftyp)),
            creator: Creator(FourCC(crea)),
            comment,
        };
        Ok(info)
    }
    fn apple_magic(&self, path: &Path, metadata: &Metadata) -> io::Result<ExtendedMetadata> {
        let magic = MAGIC
            .with_borrow(|magic| magic.file(path))
            .or::<io::Error>(Err(ErrorKind::Other.into()))?;
        let magic = magic.as_bytes();
        let (creator, file_type) = (&magic[..4], &magic[4..]);
        let info = ExtendedMetadata {
            data_len: metadata.len(),
            rsrc_len: 0,
            file_type: FileType(file_type.into()),
            creator: Creator(creator.into()),
            comment: vec![],
        };
        Ok(info)
    }
    pub fn root(&self) -> PathBuf {
        self.root.clone()
    }
    pub async fn read(&self, path: &Path) -> io::Result<FlattenedFileObject> {
        match self.read_adfs(path).await {
            Ok(ffo) => Ok(ffo),
            Err(adfs::AppleDoubleError::Io(e)) => Err(e),
            Err(e) => Err(std::io::Error::other(format!("appledouble: {e:?}"))),
        }
    }
    async fn read_adfs(&self, path: &Path) -> Result<FlattenedFileObject, adfs::AppleDoubleError> {
        let path = self.subpath(path)?;
        let file = Self::appledouble_file(&path);
        let meta = fs::metadata(&path).await?;

        let Some(mut arch) = file.open_async().await? else {
            let file = PlainFile::new(path, meta);
            return Ok(file.read().await?);
        };

        let name = path.file_name().unwrap_or_default();
        let info = self
            .read_adfs_info_fork(name.as_bytes(), &meta, &mut arch)
            .await?;
        let Some(info) = info else {
            let file = PlainFile::new(path, meta);
            return Ok(file.read().await?);
        };

        let rsrc = arch.owned_entry_reader(adfs::EntryId::ResourceFork)?;
        let data = fs::File::open(path).await?;
        let meta = data.metadata().await?;
        let data_len = meta.len();
        let data_rdr = AsyncDataSource::new(data_len, data);
        let ffo = if let Some(rsrc) = rsrc {
            let rsrc_rdr = AsyncDataSource::new(rsrc.len(), Compat::new(rsrc));
            FlattenedFileObject::with_forks(info, data_rdr, rsrc_rdr)
        } else {
            FlattenedFileObject::with_data(info, data_rdr)
        };
        Ok(ffo)
    }
    async fn read_adfs_info_fork(
        &self,
        name: &[u8],
        meta: &Metadata,
        arch: &mut adfs::AppleDoubleArchive,
    ) -> Result<Option<proto::InfoFork>, adfs::AppleDoubleError> {
        let Some(finf) = arch.finder_info().await? else {
            return Ok(None);
        };
        let created = meta.created().map(proto::FileCreatedAt::from);
        let modified = meta.modified().map(proto::FileModifiedAt::from);
        let comment = arch.comment().await?.unwrap_or_default();
        let info = proto::InfoFork {
            header: proto::InfoForkHeader {
                platform: proto::PlatformType::AppleMac,
                type_code: finf.file_type().into(),
                creator_code: finf.creator().into(),
                flags: proto::FileFlags::default(),
                platform_flags: proto::PlatformFlags::default(),
                created_at: created.unwrap_or_default(),
                modified_at: modified.unwrap_or_default(),
                name_script: proto::NameScript::default(),
            },
            filename: name.to_vec().into(),
            comment: comment.into(),
        };
        Ok(Some(info))
    }
    pub async fn write<TS: TransferStream>(
        &self,
        path: &Path,
        mut stream: FlattenedFileStream<TS>,
    ) -> io::Result<()> {
        let path = self.subpath(path)?;
        let file = Self::appledouble_file(&path);

        let info = match stream.info().await {
            Ok(info) => info,
            Err(e) => {
                return Err(io::Error::other(format!("no info fork in stream: {e:?}")));
            }
        };

        let mut data_file = file.write_data_async().await?;
        let mut rsrc_file = file.write_async().await?;

        let finf = adfs::FinderInfo {
            info: adfs::entry::FInfo {
                file_type: info.header.type_code.0.into(),
                creator: info.header.creator_code.0.into(),
                flags: adfs::entry::FinderFlags::default(),
                location: adfs::entry::Point::default(),
                folder: adfs::entry::Folder::default(),
            },
            extended: Default::default(),
        };

        let finf_data = finf.to_bytes()?;

        rsrc_file
            .add_entry_buffer(adfs::EntryId::FinderInfo, &finf_data)
            .await?;
        if info.comment.len > 0 {
            rsrc_file
                .add_entry_buffer(adfs::EntryId::Comment, &info.comment.val)
                .await?;
        }

        while let Ok(Some(fork)) = stream.next().await {
            let size = u64::from(fork.data_size);
            let mut reader = Compat::new(stream.as_mut()).take(size);
            match fork.fork_type {
                proto::ForkType::Data => {
                    data_file.seek(SeekFrom::Start(0)).await?;
                    futures_lite::io::copy(&mut reader, &mut data_file).await?;
                }
                proto::ForkType::Resource => {
                    rsrc_file
                        .add_entry(adfs::EntryId::ResourceFork, &mut reader)
                        .await?;
                }
                _ => {}
            }
        }

        rsrc_file.finish().await?;

        Ok(())
    }
    pub async fn mkdir(&self, path: &Path) -> io::Result<()> {
        let path = self.subpath(path)?;
        fs::create_dir(path).await?;
        Ok(())
    }
}

struct PlainFile {
    path: PathBuf,
    meta: Metadata,
}

impl PlainFile {
    const CREATOR_CODE: &[u8; 4] = b"dosa";
    const TYPE_CODE: &[u8; 4] = b"BINA";
    pub fn new(path: PathBuf, meta: Metadata) -> Self {
        Self { path, meta }
    }
    async fn read_info_fork(&self) -> io::Result<proto::InfoFork> {
        let type_code = proto::FileType::from(*Self::TYPE_CODE);
        let creator_code = proto::Creator::from(*Self::CREATOR_CODE);
        let name = self.path.file_name().unwrap_or_default().as_bytes();
        let created = self.meta.created().map(proto::FileCreatedAt::from);
        let modified = self.meta.modified().map(proto::FileModifiedAt::from);
        let fork = proto::InfoFork {
            header: proto::InfoForkHeader {
                platform: proto::PlatformType::MicrosoftWin,
                type_code,
                creator_code,
                created_at: created.unwrap_or_default(),
                modified_at: modified.unwrap_or_default(),
                flags: Default::default(),
                platform_flags: Default::default(),
                name_script: Default::default(),
            },
            filename: name.to_vec().into(),
            comment: vec![].into(),
        };
        Ok(fork)
    }
    async fn read_data_fork(&self) -> io::Result<AsyncDataSource> {
        let file = tokio::fs::File::open(&self.path).await?;
        let meta = file.metadata().await?;
        let len = meta.len() as u64;
        Ok(AsyncDataSource::new(len, file))
    }
    async fn read(self) -> io::Result<FlattenedFileObject> {
        let info = self.read_info_fork().await?;
        let data = self.read_data_fork().await?;
        let file = FlattenedFileObject::with_data(info, data);
        Ok(file)
    }
}
