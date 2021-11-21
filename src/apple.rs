use num_enum::{
    TryFromPrimitive,
    IntoPrimitive,
};

use nom::{
    IResult,
    combinator::verify,
    multi,
    bytes::streaming::take,
    number::streaming::{be_u32, be_u16},
};

use derive_more::From;

const MAGIC: u32 =   0x0005_1600;
const VERSION: u32 = 0x0002_0000;

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

#[derive(Debug, Copy, Clone)]
pub struct EntryDescriptor {
    pub id: u32,
    pub offset: u32,
    pub length: u32,
}

impl EntryDescriptor {
    fn to_bytes(&self) -> Vec<u8> {
        [
            self.id.to_be_bytes(),
            self.offset.to_be_bytes(),
            self.length.to_be_bytes(),
        ].into_iter()
            .flat_map(|b| b.into_iter())
            .collect()
    }
    fn from_bytes(bytes: &[u8]) -> IResult<&[u8], Self> {
        let (bytes, id) = nom::number::streaming::be_u32(bytes)?;
        let (bytes, offset) = nom::number::streaming::be_u32(bytes)?;
        let (bytes, length) = nom::number::streaming::be_u32(bytes)?;
        let descriptor = Self { id, offset, length };
        Ok((bytes, descriptor))
    }
    pub fn next_offset(&self) -> u32 {
        self.offset + self.length
    }
}

#[derive(Debug, Clone, From)]
pub struct AppleSingleHeader {
    pub descriptors: Vec<EntryDescriptor>,
}

impl AppleSingleHeader {
    pub fn calculate_size(n_entries: u32) -> u32 {
        26 + (n_entries * 12)
    }
    pub fn size(&self) -> u32 {
        Self::calculate_size(self.descriptors.len() as u32)
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let Self { descriptors } = self;
        let n_descriptors = descriptors.len() as u16;
        let entry_descriptors = descriptors.into_iter()
            .map(|d| d.to_bytes())
            .flat_map(|bytes| bytes.into_iter());
        [
            &MAGIC.to_be_bytes()[..],
            &VERSION.to_be_bytes()[..],
            &[0u8; 16][..],
            &n_descriptors.to_be_bytes()[..],
        ].into_iter()
            .flat_map(|bytes| bytes.into_iter())
            .map(|b| *b)
            .chain(entry_descriptors)
            .collect()
    }
    fn magic(bytes: &[u8]) -> IResult<&[u8], u32> {
        verify(be_u32, |magic| *magic == MAGIC)(bytes)
    }
    fn version(bytes: &[u8]) -> IResult<&[u8], u32> {
        verify(be_u32, |version| *version == VERSION)(bytes)
    }
    pub fn from_bytes(bytes: &[u8]) -> IResult<&[u8], Self> {
        let (bytes, _magic) = Self::magic(bytes)?;
        let (bytes, _version) = Self::version(bytes)?;
        let (bytes, _filler) = take(16usize)(bytes)?;
        let (bytes, n_entries) = be_u16(bytes)?;
        let (bytes, descriptors) = multi::count(
            EntryDescriptor::from_bytes,
            n_entries as usize,
        )(bytes)?;
        Ok((bytes, descriptors.into()))
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
