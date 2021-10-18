const PROTOCOL_ID: &str = r"TRTP";

struct ClientHandshakeRequest {
    protocol_id: ProtocolId,
    sub_protocol_id: SubProtocolId,
    version: Version,
    sub_version: SubVersion,
}
struct ServerHandshakeReply {
    protocol_id: ProtocolId,
    error_code: ErrorCode,
}

struct ProtocolId(u32);
struct SubProtocolId(u32);
struct Version(u16);
struct SubVersion(u16);
struct ErrorCode(u32);



struct Transaction {
}
