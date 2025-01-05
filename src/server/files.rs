use crate::{
    apple,
    protocol::{self as proto, AsyncDataSource, FlattenedFileObject},
};
use deku::prelude::*;
use derive_more::Into;
use encoding_rs::MACINTOSH;
use four_cc::FourCC;
use magic::Cookie;
use std::{
    cell::RefCell,
    ffi::OsStr,
    fs::{self, DirEntry as OsDirEntry, Metadata},
    io::{self, ErrorKind, SeekFrom},
    path::{Component, Path, PathBuf},
    time::SystemTime,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt};
use tracing::trace;

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
        &self.0 .0
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
        &self.0 .0
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

impl<'a> TryFrom<FilesContext<'a>> for DirEntry {
    type Error = std::io::Error;
    fn try_from(dirent: FilesContext<'a>) -> io::Result<Self> {
        let FilesContext { files, dirent } = dirent;
        let metadata = dirent.metadata()?;
        let path = dirent.path();
        let ExtendedMetadata {
            data_len,
            rsrc_len,
            file_type: type_code,
            creator: creator_code,
        } = if metadata.is_dir() {
            ExtendedMetadata::directory()
        } else {
            files
                .appledouble_magic(&path, &metadata)
                .or_else(|_| files.apple_magic(&path, &metadata))?
        };
        Ok(Self {
            path,
            data_len,
            rsrc_len,
            type_code,
            creator_code,
        })
    }
}

impl TryFrom<DirEntry> for proto::FileNameWithInfo {
    type Error = io::Error;
    fn try_from(value: DirEntry) -> io::Result<Self> {
        let size = value.total_size() as i32;
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
                if errors {
                    None
                } else {
                    Some(mac.to_vec())
                }
            })
            .ok_or::<Self::Error>(ErrorKind::InvalidData.into())?;
        let file_name_size = file_name.len() as i16;
        Ok(proto::FileNameWithInfo {
            file_name_size,
            file_name,
            file_size: size.into(),
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
    pub comment: String,
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
        } = magic;
        Ok(Self {
            data_len,
            rsrc_len,
            path,
            modified_at,
            created_at,
            file_type,
            creator,
            comment: String::from(""),
        })
    }
}

pub trait Files {
    fn list(&self, path: &Path) -> io::Result<Vec<DirEntry>>;
    fn get_info(&self, path: &Path) -> io::Result<FileInfo>;
}

#[derive(Debug)]
struct ExtendedMetadata {
    data_len: u64,
    rsrc_len: u64,
    file_type: FileType,
    creator: Creator,
}

impl ExtendedMetadata {
    pub fn directory() -> Self {
        Self {
            data_len: 0,
            rsrc_len: 0,
            file_type: FileType::directory(),
            creator: Creator::of_directory(),
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
    const APPLEDOUBLE_PREFIX: &'static str = "._";
    pub fn with_root<P: Into<PathBuf>>(root: P) -> io::Result<Self> {
        let root = root.into().canonicalize()?;
        let metadata = fs::metadata(&root)?;
        if metadata.is_dir() {
            Ok(Self { root })
        } else {
            Err(ErrorKind::InvalidInput.into())
        }
    }
    fn is_appledouble(dirent: &std::fs::DirEntry) -> bool {
        let name = dirent.file_name();
        let Some(name) = name.to_str() else {
            return false;
        };
        name.starts_with(Self::APPLEDOUBLE_PREFIX)
    }
    pub fn list(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        let path = self.subpath(path)?;
        fs::read_dir(path)?
            .filter_map(|e| e.ok())
            .filter(|e| !Self::is_appledouble(e))
            .map(|e| self.listing_context(e))
            .map(DirEntry::try_from)
            .collect()
    }
    pub fn get_info(&self, path: &Path) -> io::Result<FileInfo> {
        let path = self.subpath(path)?;
        let metadata = fs::metadata(&path)?;
        let info = if metadata.is_dir() {
            ExtendedMetadata::directory()
        } else {
            self.appledouble_magic(&path, &metadata)
                .or_else(|_| self.apple_magic(&path, &metadata))?
        };
        (path, metadata, info).try_into()
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
    fn appledouble_path(path: &Path) -> PathBuf {
        let basename = path.file_name().and_then(|p| p.to_str()).unwrap();
        let appledouble_basename = format!("._{basename}");
        Path::join(path.parent().unwrap(), appledouble_basename)
    }
    fn appledouble_magic(&self, path: &Path, metadata: &Metadata) -> io::Result<ExtendedMetadata> {
        let path = Self::appledouble_path(path);
        let mut ad_file = std::fs::OpenOptions::new()
            .read(true)
            .write(false)
            .create(false)
            .append(false)
            .open(path)?;
        let (_, header) = crate::apple::AppleSingleHeader::from_reader((&mut ad_file, 0))?;
        let rsrc_len = header.resource_fork().map(|rsrc| rsrc.length).unwrap_or(0) as u64;
        // FIXME: this assumes FINF is right after header
        let (_, finf) = crate::apple::FinderInfo::from_reader((&mut ad_file, 0))?;

        let info = ExtendedMetadata {
            data_len: metadata.len(),
            rsrc_len,
            file_type: FileType((&finf.file_type.0 .0).into()),
            creator: Creator((&finf.creator.0 .0).into()),
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
        };
        Ok(info)
    }
    fn listing_context(&self, dirent: OsDirEntry) -> FilesContext {
        FilesContext {
            files: self,
            dirent,
        }
    }
    pub fn root(&self) -> PathBuf {
        self.root.clone()
    }
    pub async fn read(&self, path: &Path) -> io::Result<FlattenedFileObject> {
        let path = self.subpath(path)?;
        let appledouble_path = Self::appledouble_path(&path);
        let file = if appledouble_path.is_file() {
            let file = AppleDoubleFile::new(path, appledouble_path);
            file.read().await
        } else {
            let file = PlainFile::new(path);
            file.read().await
        }?;
        Ok(file)
    }
}

impl Files for OsFiles {
    fn list(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        OsFiles::list(self, path)
    }
    fn get_info(&self, path: &Path) -> io::Result<FileInfo> {
        OsFiles::get_info(self, path)
    }
}

struct FilesContext<'a> {
    files: &'a OsFiles,
    dirent: OsDirEntry,
}

struct PlainFile {
    path: PathBuf,
}

impl PlainFile {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
    async fn read_info_fork(&self) -> io::Result<proto::InfoFork> {
        let finf = apple::FinderInfo::windows_file();
        let type_code = proto::FileType::from(finf.file_type);
        let creator_code = proto::Creator::from(finf.creator);
        let filename = self
            .path
            .file_name()
            .expect("no filename")
            .to_str()
            .expect("no string filename");
        let (file_name, _, failed) = MACINTOSH.encode(filename);
        if failed {
            panic!("bad filename");
        }
        let file_name = file_name.into_owned();
        let fork = proto::InfoFork {
            platform: proto::PlatformType::MicrosoftWin,
            type_code,
            creator_code,
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

#[derive(Into)]
struct AppleDoubleFile {
    path: PathBuf,
    appledouble_path: PathBuf,
}

impl AppleDoubleFile {
    pub fn new(path: PathBuf, appledouble_path: PathBuf) -> Self {
        Self {
            path,
            appledouble_path,
        }
    }
    async fn read_appledouble_header_stub(
        mut reader: impl AsyncRead + Unpin,
    ) -> io::Result<apple::AppleSingleHeaderStub> {
        let mut buf = [0u8; apple::AppleSingleHeaderStub::calculate_size()];
        reader.read_exact(&mut buf).await?;
        let stub = apple::AppleSingleHeaderStub::try_from(&buf[..])?;
        Ok(stub)
    }
    async fn seek_to(
        mut reader: impl AsyncSeekExt + Unpin,
        entry: apple::EntryDescriptor,
    ) -> io::Result<()> {
        reader.seek(SeekFrom::Start(entry.offset as u64)).await?;
        Ok(())
    }
    async fn read_finf(
        mut reader: impl AsyncRead + AsyncSeek + Unpin,
        header: &apple::AppleSingleHeader,
    ) -> io::Result<Option<apple::FinderInfo>> {
        let Some(finf_entry) = header.finder_info() else {
            return Ok(None);
        };
        Self::seek_to(&mut reader, finf_entry).await?;
        let mut buf = [0u8; apple::FinderInfo::calculate_size()];
        reader.read_exact(&mut buf).await?;
        let finf = apple::FinderInfo::try_from(&buf[..])?;
        Ok(Some(finf))
    }
    async fn read_appledouble_header(
        mut reader: impl AsyncRead + Unpin,
    ) -> io::Result<apple::AppleSingleHeader> {
        let stub = Self::read_appledouble_header_stub(&mut reader).await?;
        let mut entries = vec![];
        for _ in 0..stub.n_descriptors {
            let mut buf = [0u8; apple::EntryDescriptor::calculate_size()];
            reader.read_exact(&mut buf).await?;
            entries.push(apple::EntryDescriptor::try_from(&buf[..])?);
        }
        Ok(apple::AppleSingleHeader::new_double(entries))
    }
    async fn read_comment(
        &self,
        header: &apple::AppleSingleHeader,
        mut reader: impl AsyncRead + AsyncSeek + Unpin,
    ) -> io::Result<Vec<u8>> {
        let Some(entry) = header.entry(apple::EntryId::Comment) else {
            return Ok(vec![]);
        };
        Self::seek_to(&mut reader, entry).await?;
        let len = entry.length as usize;
        let mut comment = vec![0u8; len];
        reader.read_exact(&mut comment[..len]).await?;
        Ok(comment)
    }
    async fn read_info_fork(&self) -> io::Result<proto::InfoFork> {
        let mut file = tokio::fs::File::open(&self.appledouble_path).await?;
        let header = Self::read_appledouble_header(&mut file).await?;
        let finf = Self::read_finf(&mut file, &header)
            .await?
            .unwrap_or_else(apple::FinderInfo::windows_file);
        let type_code = proto::FileType::from(finf.file_type);
        let creator_code = proto::Creator::from(finf.creator);
        let filename = self
            .path
            .file_name()
            .expect("no filename")
            .to_str()
            .expect("no string filename");
        let (file_name, _, failed) = MACINTOSH.encode(filename);
        if failed {
            panic!("bad filename");
        }
        let comment = self.read_comment(&header, &mut file).await?;
        let platform_flags = u16::from(finf.flags) as u32;
        let file_name = file_name.into_owned();
        let fork = proto::InfoFork {
            platform: proto::PlatformType::AppleMac,
            type_code,
            creator_code,
            flags: Default::default(),
            platform_flags: proto::PlatformFlags::from(platform_flags),
            created_at: Default::default(),
            modified_at: Default::default(),
            name_script: Default::default(),
            name_len: file_name.len() as i16,
            file_name,
            comment_len: comment.len() as i16,
            comment,
        };
        Ok(fork)
    }
    async fn read_data_fork(&self) -> io::Result<AsyncDataSource> {
        let file = tokio::fs::File::open(&self.path).await?;
        let meta = file.metadata().await?;
        let len = meta.len() as u64;
        Ok(AsyncDataSource::new(len, file))
    }
    async fn read_rsrc_fork(&self) -> io::Result<Option<AsyncDataSource>> {
        let mut file = tokio::fs::File::open(&self.appledouble_path).await?;
        let header = Self::read_appledouble_header(&mut file).await?;
        let Some(rsrc_entry) = header.resource_fork() else {
            return Ok(None);
        };
        trace!("have rsrc entry {rsrc_entry:?}");
        file.seek(SeekFrom::Start(rsrc_entry.offset as u64)).await?;
        let len = rsrc_entry.length as u64;
        Ok(Some(AsyncDataSource::new(len, file)))
    }
    async fn read(self) -> io::Result<FlattenedFileObject> {
        let info = self.read_info_fork().await?;
        let data = self.read_data_fork().await?;
        let rsrc = self.read_rsrc_fork().await?;
        let file = if let Some(rsrc) = rsrc {
            FlattenedFileObject::with_forks(info, data, rsrc)
        } else {
            FlattenedFileObject::with_data(info, data)
        };
        Ok(file)
    }
}
