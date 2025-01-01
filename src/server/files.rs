use crate::protocol as proto;
use deku::prelude::*;
use derive_more::Into;
use encoding_rs::MACINTOSH;
use four_cc::FourCC;
use magic::Cookie;
use std::{
    ffi::OsStr,
    fs::{self, DirEntry as OsDirEntry, Metadata},
    io::{self, ErrorKind},
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

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

pub struct Comment(String);

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

#[derive(Debug)]
pub struct OsFiles {
    root: PathBuf,
    magic: Cookie<magic::cookie::Load>,
}

impl OsFiles {
    const APPLEDOUBLE_PREFIX: &'static str = "._";
    pub fn with_root<P: Into<PathBuf>>(root: P) -> io::Result<Self> {
        let root = root.into().canonicalize()?;
        let metadata = fs::metadata(&root)?;
        let magic = Cookie::open(magic::cookie::Flags::APPLE)
            .or::<io::Error>(Err(ErrorKind::Other.into()))?;
        let magic = magic
            .load(&Default::default())
            .or::<io::Error>(Err(ErrorKind::Other.into()))?;
        if metadata.is_dir() {
            Ok(Self { root, magic })
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
            .map(|e| DirEntry::try_from(e))
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
    fn appledouble_magic(&self, path: &Path, metadata: &Metadata) -> io::Result<ExtendedMetadata> {
        let basename = path.file_name().and_then(|p| p.to_str()).unwrap();
        let appledouble_basename = format!("._{basename}");
        let path = Path::join(path.parent().unwrap(), appledouble_basename);
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
        let magic = self
            .magic
            .file(path)
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
