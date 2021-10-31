use super::{
    ErrorCode,
    HotlineProtocol,
    BIResult,
    be_i16,
    be_i32,
    bytes,
};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProtocolId(i32);

impl HotlineProtocol for ProtocolId {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i32(bytes)?;
        Ok((bytes, Self(value)))
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SubProtocolId(i32);

impl HotlineProtocol for SubProtocolId {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i32(bytes)?;
        Ok((bytes, Self(value)))
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub i16);

impl HotlineProtocol for Version {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i16(bytes)?;
        Ok((bytes, Self(value)))
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SubVersion(pub i16);

impl HotlineProtocol for SubVersion {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i16(bytes)?;
        Ok((bytes, Self(value)))
    }
}

#[derive(Debug)]
pub struct ClientHandshakeRequest {
    pub sub_protocol_id: SubProtocolId,
    pub version: Version,
    pub sub_version: SubVersion,
}

impl HotlineProtocol for ClientHandshakeRequest {
    fn into_bytes(self) -> Vec<u8> {
        let Self {
            sub_protocol_id,
            version,
            sub_version,
            ..
        } = self;
        let protocol_id = &b"TRTP"[..];
        let sub_protocol_id = &sub_protocol_id.into_bytes();
        let version = &version.into_bytes();
        let sub_version = &sub_version.into_bytes();
        [
            protocol_id,
            sub_protocol_id,
            version,
            sub_version,
        ].into_iter()
            .flat_map(|bytes| bytes.iter())
            .map(|b| *b)
            .collect()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, _) = bytes::streaming::tag(b"TRTP")(bytes)?;
        let (bytes, sub_protocol_id) = SubProtocolId::from_bytes(bytes)?;
        let (bytes, version) = Version::from_bytes(bytes)?;
        let (bytes, sub_version) = SubVersion::from_bytes(bytes)?;
        let handshake = Self {
            sub_protocol_id,
            version,
            sub_version,
        };
        Ok((bytes, handshake))
    }
}

#[derive(Debug)]
pub struct ServerHandshakeReply {
    error_code: ErrorCode,
}

impl ServerHandshakeReply {
    pub fn ok() -> Self {
        Self { error_code: ErrorCode(0) }
    }
}

impl HotlineProtocol for ServerHandshakeReply {
    fn into_bytes(self) -> Vec<u8> {
        let head = &b"TRTP"[..];
        let error = &self.error_code.into_bytes();
        [head, error].into_iter()
            .flat_map(|bytes| bytes.iter())
            .map(|b| *b)
            .collect()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, _) = bytes::streaming::tag(b"TRTP")(bytes)?;
        let (bytes, error_code) = ErrorCode::from_bytes(bytes)?;
        Ok((bytes, ServerHandshakeReply { error_code }))
    }
}
