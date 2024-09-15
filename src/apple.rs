use num_enum::{
    TryFromPrimitive,
    IntoPrimitive,
};

use deku::prelude::*;

use derive_more::From;

#[derive(
    Debug,
    Copy, Clone,
    Eq, PartialEq,
    Ord, PartialOrd,
    TryFromPrimitive, IntoPrimitive,
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
}

#[derive(Debug, Clone, From, DekuRead, DekuWrite)]
#[deku(magic = b"\x00\x05\x16\x00\x00\x02\x00\x00")]
pub struct AppleSingleHeader {
    #[deku(pad_bytes_before = "16", update = "self.descriptors.len() as u16", endian = "big")]
    pub n_descriptors: u16,
    #[deku(count = "n_descriptors")]
    pub descriptors: Vec<EntryDescriptor>,
}

impl AppleSingleHeader {
    pub fn new(descriptors: Vec<EntryDescriptor>) -> Self {
        Self {
            n_descriptors: descriptors.len() as u16,
            descriptors,
        }
    }
    pub fn calculate_size(n_entries: u32) -> u32 {
        26 + (n_entries * 12)
    }
    pub fn data_fork(&self) -> Option<EntryDescriptor> {
        self.descriptors.iter()
            .filter(|d| EntryId::try_from(d.id) == Ok(EntryId::DataFork))
            .cloned()
            .next()
    }
    pub fn resource_fork(&self) -> Option<EntryDescriptor> {
        self.descriptors.iter()
            .filter(|d| EntryId::try_from(d.id) == Ok(EntryId::ResourceFork))
            .cloned()
            .next()
    }
}
