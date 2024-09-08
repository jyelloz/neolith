use super::{
    ErrorCode,
    ReferenceNumber,
    DataSize,
    DekuHotlineProtocol,
};
use deku::prelude::*;

use derive_more::{From, Into};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct ProtocolId(i32);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct SubProtocolId(i32);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct Version(pub i16);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct SubVersion(pub i16);

#[derive(Debug, DekuRead, DekuWrite)]
#[deku(magic = b"TRTP")]
pub struct ClientHandshakeRequest {
    pub sub_protocol_id: SubProtocolId,
    pub version: Version,
    pub sub_version: SubVersion,
}

#[derive(Debug, DekuRead, DekuWrite)]
#[deku(magic = b"TRTP")]
pub struct ServerHandshakeReply {
    error_code: ErrorCode,
}

impl ServerHandshakeReply {
    pub fn ok() -> Self {
        Self { error_code: ErrorCode(0) }
    }
}

#[derive(Debug, DekuRead, DekuWrite)]
#[deku(magic = b"HTXF")]
pub struct TransferHandshake {
    pub reference: ReferenceNumber,
    pub size: Option<DataSize>,
    padding: [u8; 4],
}

impl TransferHandshake {
    pub fn is_upload(&self) -> bool {
        self.size.is_some()
    }
}

impl DekuHotlineProtocol for ClientHandshakeRequest {}
impl DekuHotlineProtocol for ServerHandshakeReply {}
impl DekuHotlineProtocol for TransferHandshake {}

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
        ClientHandshakeRequest::try_from(CLIENT_HANDSHAKE)
            .expect("could not parse client handshake");
    }

    #[test]
    fn parse_server_handshake() {
        let handshake = ServerHandshakeReply::try_from(SERVER_HANDSHAKE)
            .expect("could not parse server handshake");
        assert_eq!(
            handshake.error_code,
            ErrorCode(0),
        );
    }
}
