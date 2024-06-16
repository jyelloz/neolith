use std::{
    fs::{self, DirEntry as OsDirEntry, Metadata},
    io::{self, Result, ErrorKind},
    path::{Path, PathBuf, Component},
    time::SystemTime,
};
use encoding_rs::MACINTOSH;
use magic::Cookie;
use four_cc::FourCC;
use crate::protocol as proto;

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

pub struct Comment(String);

pub struct DirEntry {
    pub path: PathBuf,
    pub size: u64,
    pub type_code: FileType,
    pub creator_code: Creator,
}

impl <'a> TryFrom<FilesContext<'a>> for DirEntry {
    type Error = std::io::Error;
    fn try_from(dirent: FilesContext<'a>) -> Result<Self> {
        let FilesContext { files, dirent } = dirent;
        let metadata = dirent.metadata()?;
        let path = dirent.path();
        let size = metadata.len();
        let (type_code, creator_code) = if metadata.is_dir() {
            (FileType::directory(), Creator::default())
        } else {
            files.apple_magic(&path)?
        };
        Ok(
            Self {
                path,
                size,
                type_code,
                creator_code,
            }
        )
    }
}

impl From<DirEntry> for proto::FileNameWithInfo {
    fn from(value: DirEntry) -> Self {
        let DirEntry {
            creator_code,
            type_code,
            size,
            path,
            ..
        } = value;
        let file_name = path.file_name()
            .and_then(|s| s.to_str())
            .map(|s| MACINTOSH.encode(s).0)
            .map(|b| b.to_vec())
            .unwrap_or(vec![]);
        proto::FileNameWithInfo {
            file_name,
            file_size: (size as i32).into(),
            creator: (*creator_code.bytes()).into(),
            file_type: (*type_code.bytes()).into(),
            name_script: 0.into(),
        }
    }
}

#[derive(Debug)]
pub struct FileInfo {
    pub path: PathBuf,
    pub size: u64,
    pub type_code: FileType,
    pub creator_code: Creator,
    pub comment: String,
    pub created_at: SystemTime,
    pub modified_at: SystemTime,
}

impl TryFrom<(PathBuf, Metadata, FileType, Creator)> for FileInfo {
    type Error = std::io::Error;
    fn try_from(
        (path, metadata, file_type, creator):
        (PathBuf, Metadata, FileType, Creator)
    ) -> Result<Self> {
        let modified_at = metadata.modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let created_at = metadata.created()
            .unwrap_or(modified_at);
        let (type_code, creator_code) = if metadata.is_dir() {
            (FileType::directory(), Creator::of_directory())
        } else {
            (file_type, creator)
        };
        let size = metadata.len();
        Ok(
            Self {
                size,
                path,
                modified_at,
                created_at,
                type_code,
                creator_code,
                comment: String::from(""),
            }
        )
    }
}

pub trait Files {
    fn list(&self, path: &Path) -> Result<Vec<DirEntry>>;
    fn get_info(&self, path: &Path) -> Result<FileInfo>;
}

#[derive(Debug)]
pub struct OsFiles {
    root: PathBuf,
    magic: Cookie<magic::cookie::Load>,
}

impl OsFiles {
    pub fn with_root<P: Into<PathBuf>>(root: P) -> Result<Self> {
        let root = root.into().canonicalize()?;
        let metadata = fs::metadata(&root)?;
        let magic = Cookie::open(magic::cookie::Flags::APPLE)
            .or::<io::Error>(Err(ErrorKind::Other.into()))?;
        let magic = magic.load(&Default::default())
            .or::<io::Error>(Err(ErrorKind::Other.into()))?;
        if metadata.is_dir() {
            Ok(Self { root, magic })
        } else {
            Err(ErrorKind::InvalidInput.into())
        }
    }
    pub fn list(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let path = self.subpath(path)?;
        fs::read_dir(path)?
            .map(|e| e.map(|e| self.listing_context(e)))
            .map(|e| e.and_then(DirEntry::try_from))
            .collect()
    }
    pub fn get_info(&self, path: &Path) -> Result<FileInfo> {
        let path = self.subpath(path)?;
        let metadata = fs::metadata(&path)?;
        let (file_type, creator) = if metadata.is_dir() {
            (FileType::directory(), Creator::of_directory())
        } else {
            self.apple_magic(&path)?
        };
        (path, metadata, file_type, creator).try_into()
    }
    fn validate_path(path: &Path) -> Result<&Path> {
        let complex = path.components()
            .any(|p| p == Component::ParentDir);
        if complex {
            return Err(ErrorKind::InvalidInput.into());
        }
        Ok(path)
    }
    fn subpath(&self, path: &Path) -> Result<PathBuf> {
        let Self { root, .. } = self;
        let path = Self::validate_path(path)?;
        let subpath = root.components()
            .chain(path.components())
            .collect();
        Ok(subpath)
    }
    fn apple_magic(&self, path: &Path) -> Result<(FileType, Creator)> {
        let magic = self.magic.file(path)
            .or::<io::Error>(Err(ErrorKind::Other.into()))?;
        let magic = magic.as_bytes();
        let (creator, file_type) = (&magic[..4], &magic[4..]);
        Ok((
            FileType(file_type.into()),
            Creator(creator.into())),
        )
    }
    fn listing_context(&self, dirent: OsDirEntry) -> FilesContext {
        FilesContext {
            files: self,
            dirent,
        }
    }
}

impl Files for OsFiles {
    fn list(&self, path: &Path) -> Result<Vec<DirEntry>> {
        OsFiles::list(self, path)
    }
    fn get_info(&self, path: &Path) -> Result<FileInfo> {
        OsFiles::get_info(self, path)
    }
}

struct FilesContext<'a> {
    files: &'a OsFiles,
    dirent: OsDirEntry,
}
