use std::{
    fs::{
        DirEntry as OsDirEntry,
        Metadata,
    },
    io::{Result, ErrorKind},
    path::{Path, PathBuf},
    time::SystemTime,
};

use four_cc::FourCC;

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

impl TryFrom<OsDirEntry> for DirEntry {
    type Error = std::io::Error;
    fn try_from(dirent: OsDirEntry) -> Result<Self> {
        let metadata = dirent.metadata()?;
        let path = dirent.path();
        let size = metadata.len();
        let (type_code, creator_code) = if metadata.is_dir() {
            (FileType::directory(), Creator::default())
        } else {
            (FileType::default(), Creator::default())
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

pub struct FileInfo {
    pub path: PathBuf,
    pub size: u64,
    pub type_code: FileType,
    pub creator_code: Creator,
    pub comment: String,
    pub created_at: SystemTime,
    pub modified_at: SystemTime,
}

impl TryFrom<(PathBuf, Metadata)> for FileInfo {
    type Error = std::io::Error;
    fn try_from((path, metadata): (PathBuf, Metadata)) -> Result<Self> {
        let modified_at = metadata.modified()?;
        let created_at = metadata.created()?;
        let (type_code, creator_code) = if metadata.is_dir() {
            (FileType::directory(), Creator::of_directory())
        } else {
            (FileType::default(), Creator::default())
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

pub struct OsFiles(PathBuf);

impl OsFiles {
    pub fn with_root(root: PathBuf) -> Result<Self> {
        let metadata = std::fs::metadata(&root)?;
        if metadata.is_dir() {
            Ok(Self(root))
        } else {
            Err(ErrorKind::InvalidInput.into())
        }
    }
    pub fn list(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let path = self.subpath(path);
        std::fs::read_dir(path)?
            .map(|e| e.and_then(DirEntry::try_from))
            .collect()
    }
    pub fn get_info(&self, path: &Path) -> Result<FileInfo> {
        let path = self.subpath(path);
        let metadata = std::fs::metadata(&path)?;
        Err(ErrorKind::NotFound.into())
    }
    fn subpath(&self, path: &Path) -> PathBuf {
        let Self(root) = self;
        root.components()
            .chain(path.components())
            .collect()
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
