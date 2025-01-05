use num_enum::{IntoPrimitive, TryFromPrimitive};

use deku::prelude::*;

use derive_more::From;

#[derive(
    Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, TryFromPrimitive, IntoPrimitive, From,
)]
#[repr(u32)]
pub enum EntryId {
    DataFork = 1,
    ResourceFork,
    RealName,
    Comment,
    IconBW,
    IconColor,
    FileDatesInfo = 8,
    FinderInfo,
    MacintoshFileInfo,
    ProDOSFileInfo,
    MSDOSFileInfo,
    ShortName,
    AFPFileInfo,
    DirectoryID,
}

#[derive(Debug, Copy, Clone, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct EntryDescriptor {
    pub id: u32,
    pub offset: u32,
    pub length: u32,
}

impl EntryDescriptor {
    pub fn next_offset(&self) -> u32 {
        self.offset + self.length
    }
    pub fn entry_id(&self) -> Option<EntryId> {
        EntryId::try_from(self.id).ok()
    }
    pub fn finf(self) -> Option<Self> {
        if matches!(self.entry_id(), Some(EntryId::FinderInfo)) {
            Some(self)
        } else {
            None
        }
    }
    pub fn rsrc(self) -> Option<Self> {
        if matches!(self.entry_id(), Some(EntryId::ResourceFork)) {
            Some(self)
        } else {
            None
        }
    }
    pub const fn calculate_size() -> usize {
        4 + 4 + 4
    }
}

#[derive(Debug, Default, Clone, From, PartialEq, Eq, DekuRead, DekuWrite)]
#[deku(id_type = "u32", endian = "big")]
enum AppleSingleDoubleMagic {
    #[default]
    #[deku(id = 0x0005_1600u32)]
    Single,
    #[deku(id = 0x0005_1607u32)]
    Double,
}

#[derive(Debug, Clone, From, PartialEq, Eq, DekuRead, DekuWrite)]
#[deku(endian = "big")]
struct AppleSingleVersion(u32);

impl Default for AppleSingleVersion {
    fn default() -> Self {
        Self(0x2_0000)
    }
}

#[derive(Debug, Clone, From, DekuRead, DekuWrite)]
pub struct AppleSingleHeaderStub {
    magic: AppleSingleDoubleMagic,
    version: AppleSingleVersion,
    #[deku(pad_bytes_before = "16", endian = "big")]
    pub n_descriptors: u16,
}

impl AppleSingleHeaderStub {
    pub const fn calculate_size() -> usize {
        4 + 4 + 16 + 2
    }
}

#[derive(Debug, Clone, From, DekuRead, DekuWrite)]
pub struct AppleSingleHeader {
    magic: AppleSingleDoubleMagic,
    version: AppleSingleVersion,
    #[deku(
        pad_bytes_before = "16",
        update = "self.descriptors.len() as u16",
        endian = "big"
    )]
    pub n_descriptors: u16,
    #[deku(count = "n_descriptors")]
    pub descriptors: Vec<EntryDescriptor>,
}

impl AppleSingleHeader {
    pub fn new_single(descriptors: Vec<EntryDescriptor>) -> Self {
        Self {
            magic: AppleSingleDoubleMagic::Single,
            version: AppleSingleVersion::default(),
            n_descriptors: descriptors.len() as u16,
            descriptors,
        }
        .compute_internal_offsets()
    }
    pub fn new_double(descriptors: Vec<EntryDescriptor>) -> Self {
        Self {
            magic: AppleSingleDoubleMagic::Double,
            version: AppleSingleVersion::default(),
            n_descriptors: descriptors.len() as u16,
            descriptors,
        }
        .compute_internal_offsets()
    }
    fn compute_internal_offsets(mut self) -> Self {
        let mut offset = Self::calculate_size(self.n_descriptors as u32);
        for descriptor in &mut self.descriptors {
            descriptor.offset = offset;
            offset += descriptor.length;
        }
        self
    }
    pub fn calculate_size(n_entries: u32) -> u32 {
        26 + (n_entries * 12)
    }
    pub fn data_fork(&self) -> Option<EntryDescriptor> {
        self.descriptors
            .iter()
            .filter(|d| EntryId::try_from(d.id) == Ok(EntryId::DataFork))
            .cloned()
            .next()
    }
    pub fn resource_fork(&self) -> Option<EntryDescriptor> {
        self.descriptors
            .iter()
            .filter(|d| EntryId::try_from(d.id) == Ok(EntryId::ResourceFork))
            .cloned()
            .next()
    }
    pub fn finder_info(&self) -> Option<EntryDescriptor> {
        self.descriptors
            .iter()
            .filter(|d| EntryId::try_from(d.id) == Ok(EntryId::FinderInfo))
            .cloned()
            .next()
    }
}

#[derive(DekuRead, DekuWrite, Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinderInfo {
    pub file_type: FileType,
    pub creator: Creator,
    pub flags: FinderFlags,
    pub location: Point,
    #[deku(pad_bytes_after = "16")]
    pub folder: Folder,
}

impl FinderInfo {
    pub const fn calculate_size() -> usize {
        4 + 4 + 2 + 4 + 2 + 16
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite, From)]
pub struct FourCC(pub [u8; 4]);
#[derive(Debug, DekuRead, DekuWrite, Clone, Copy, PartialEq, Eq, From)]
pub struct FileType(pub FourCC);
#[derive(Debug, DekuRead, DekuWrite, Clone, Copy, PartialEq, Eq)]
pub struct Creator(pub FourCC);

#[derive(Default, Debug, DekuRead, DekuWrite, Clone, Copy, PartialEq, Eq)]
#[deku(endian = "big")]
pub struct FinderFlags {
    #[deku(bits = "1")]
    pub is_alias: bool,
    #[deku(bits = "1")]
    pub is_invisible: bool,
    #[deku(bits = "1")]
    pub has_bundle: bool,
    #[deku(bits = "1")]
    pub name_locked: bool,
    #[deku(bits = "1")]
    pub is_stationery: bool,
    #[deku(bits = "1", pad_bits_after = "1")]
    pub has_custom_icon: bool,

    #[deku(bits = "1")]
    pub has_been_inited: bool,
    #[deku(bits = "1")]
    pub has_no_inits: bool,
    #[deku(bits = "1")]
    pub is_shared: bool,
    #[deprecated]
    #[deku(bits = "1", pad_bits_after = "1")]
    pub requires_switch_launch: bool,

    #[deku(bits = "3")]
    pub color: u8,
    #[deku(bits = "1")]
    #[deprecated]
    pub is_on_desktop: bool,
}

#[derive(Debug, DekuRead, DekuWrite, Default, Clone, Copy, PartialEq, Eq)]
#[deku(endian = "big")]
pub struct Point {
    pub vertical: i16,
    pub horizontal: i16,
}

#[derive(Debug, DekuRead, DekuWrite, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[deku(endian = "big")]
pub struct Folder(#[deku(bits = "16")] u16);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_write_appledouble() -> std::io::Result<()> {
        let start = AppleSingleHeader::calculate_size(2);
        let rsrc = [0u8; 128];
        let finf_descriptor = EntryDescriptor {
            id: EntryId::FinderInfo.into(),
            offset: start,
            length: FinderInfo::calculate_size() as u32,
        };
        let rsrc_descriptor = EntryDescriptor {
            id: EntryId::ResourceFork.into(),
            offset: finf_descriptor.next_offset(),
            length: rsrc.len() as u32,
        };
        let entries = vec![finf_descriptor, rsrc_descriptor];
        let hdr = AppleSingleHeader::new_double(entries);
        eprintln!("{hdr:?}");

        let _finf = FinderInfo {
            file_type: FileType(FourCC(*b"APPL")),
            creator: Creator(FourCC(*b"ttxt")),
            flags: FinderFlags::default(),
            location: Point::default(),
            folder: Folder::default(),
        };

        Ok(())
    }

    #[test]
    fn test_applesingle_magic() {
        let magic = [0x00, 0x05, 0x16, 0x00];
        let parsed = AppleSingleDoubleMagic::try_from(magic.as_slice());
        eprintln!("parsed magic as {parsed:?}");
        assert_eq!(parsed, Ok(AppleSingleDoubleMagic::Single));
    }

    #[test]
    fn test_appledouble_magic() {
        let magic = [0x00, 0x05, 0x16, 0x07];
        let parsed = AppleSingleDoubleMagic::try_from(magic.as_slice());
        eprintln!("parsed magic as {parsed:?}");
        assert_eq!(parsed, Ok(AppleSingleDoubleMagic::Double));
    }

    #[test]
    fn test_appledouble_magic_invalid() {
        let magic = [0u8; 4];
        let parsed = AppleSingleDoubleMagic::try_from(magic.as_slice());
        assert!(parsed.is_err())
    }
}
