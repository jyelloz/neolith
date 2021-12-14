use super::{
    ErrorCode,
    ReferenceNumber,
    DataSize,
    HotlineProtocol,
    BIResult,
    be_i16,
    be_i32,
    bytes,
};

use derive_more::{From, Into};

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

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, From, Into)]
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
        ].concat()
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

const TRTP: &[u8] = b"TRTP";

impl HotlineProtocol for ServerHandshakeReply {
    fn into_bytes(self) -> Vec<u8> {
        let error = &self.error_code.into_bytes();
        [TRTP, error].concat()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, _) = bytes::streaming::tag(TRTP)(bytes)?;
        let (bytes, error_code) = ErrorCode::from_bytes(bytes)?;
        Ok((bytes, Self { error_code }))
    }
}

#[derive(Debug)]
pub struct DownloadHandshakeRequest {
    pub reference: ReferenceNumber,
    pub size: DataSize,
}

const HTXF: &[u8; 4] = b"HTXF";

impl HotlineProtocol for DownloadHandshakeRequest {
    fn into_bytes(self) -> Vec<u8> {
        let Self { reference, size } = self;
        let reference: i32 = reference.into();
        let reference = reference.to_be_bytes();
        let size: i32 = size.into();
        let size = size.to_be_bytes();
        [
            &HTXF[..],
            &reference[..],
            &size[..],
            &[0u8; 4][..],
        ].concat()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, _) = bytes::streaming::tag(HTXF)(bytes)?;
        let (bytes, reference) = ReferenceNumber::from_bytes(bytes)?;
        let (bytes, size) = DataSize::from_bytes(bytes)?;
        let (bytes, _) = bytes::streaming::take(4usize)(bytes)?;
        Ok((bytes, Self { reference, size }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static CLIENT_HANDSHAKE: &'static [u8] = &[
        0x54, 0x52, 0x54, 0x50, 0x48, 0x4f, 0x54, 0x4c,
        0x00, 0x01, 0x00, 0x02,
    ];

    static SERVER_HANDSHAKE: &'static [u8] = &[
        0x54, 0x52, 0x54, 0x50, 0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn parse_client_handshake() {

        let (tail, _handshake) = ClientHandshakeRequest::from_bytes(CLIENT_HANDSHAKE)
            .expect("could not parse client handshake");

        assert!(tail.is_empty());

    }

    #[test]
    fn parse_server_handshake() {

        let (tail, handshake) = ServerHandshakeReply::from_bytes(SERVER_HANDSHAKE)
            .expect("could not parse server handshake");

        assert!(tail.is_empty());

        assert_eq!(
            handshake.error_code,
            ErrorCode(0),
        );

    }
}
