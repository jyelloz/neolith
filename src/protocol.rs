use nom::{
    self,
    IResult,
    bytes,
    number::streaming::{
        be_i32,
        be_i16,
        be_i8,
    },
};

struct ClientHandshakeRequest {
    sub_protocol_id: SubProtocolId,
    version: Version,
    sub_version: SubVersion,
}
struct ServerHandshakeReply {
    error_code: ErrorCode,
}

struct ProtocolId(i32);
struct SubProtocolId(i32);
struct Version(i16);
struct SubVersion(i16);
struct ErrorCode(i32);


struct TransactionHeader {
    flags: Flags,
    is_reply: IsReply,
    _type: Type,
    id: Id,
    error_code: ErrorCode,
    total_size: TotalSize,
    data_size: DataSize,
}

struct Flags(i8);
struct IsReply(i8);
struct Type(i16);
struct Id(i32);
struct TotalSize(i32);
struct DataSize(i32);

struct ParameterRecord {
    field_id: FieldId,
    field_size: FieldSize,
    field_data: Vec<u8>,
}

struct FieldId(i16);
struct FieldSize(i16);

struct TransactionBody {
}

fn sub_protocol_id(input: &[u8]) -> IResult<&[u8], SubProtocolId> {
    let (input, id) = be_i32(input)?;
    Ok((input, SubProtocolId(id)))
}

fn version(input: &[u8]) -> IResult<&[u8], Version> {
    let (input, version) = be_i16(input)?;
    Ok((input, Version(version)))
}

fn sub_version(input: &[u8]) -> IResult<&[u8], SubVersion> {
    let (input, sub_version) = be_i16(input)?;
    Ok((input, SubVersion(sub_version)))
}

fn client_handshake_request(input: &[u8]) -> IResult<&[u8], ClientHandshakeRequest> {
    let (input, _) = bytes::streaming::tag(r"TRTP")(input)?;
    let (input, sub_protocol_id) = sub_protocol_id(input)?;
    let (input, version) = version(input)?;
    let (input, sub_version) = sub_version(input)?;
    Ok((
        input,
        ClientHandshakeRequest {
            sub_protocol_id,
            version,
            sub_version,
        },
    ))
}

fn error_code(input: &[u8]) -> IResult<&[u8], ErrorCode> {
    be_i32(input).map(
        |(input, code)| (input, ErrorCode(code))
    )
}

fn server_handshake_reply(input: &[u8]) -> IResult<&[u8], ServerHandshakeReply> {
    let (input, _) = bytes::streaming::tag(r"TRTP")(input)?;
    let (input, error_code) = error_code(input)?;
    Ok((input, ServerHandshakeReply { error_code }))
}

fn flags(input: &[u8]) -> IResult<&[u8], Flags> {
    be_i8(input).map(|(input, flags)| (input, Flags(flags)))
}

fn is_reply(input: &[u8]) -> IResult<&[u8], IsReply> {
    be_i8(input).map(|(input, is_reply)| (input, IsReply(is_reply)))
}

fn id(input: &[u8]) -> IResult<&[u8], Id> {
    be_i32(input).map(|(input, id)| (input, Id(id)))
}

fn _type(input: &[u8]) -> IResult<&[u8], Type> {
    be_i16(input).map(|(input, _type)| (input, Type(_type)))
}

fn total_size(input: &[u8]) -> IResult<&[u8], TotalSize> {
    be_i32(input).map(|(input, size)| (input, TotalSize(size)))
}

fn data_size(input: &[u8]) -> IResult<&[u8], DataSize> {
    be_i32(input).map(|(input, size)| (input, DataSize(size)))
}

fn transaction_header(input: &[u8]) -> IResult<&[u8], TransactionHeader> {

    let (input, flags) = flags(input)?;
    let (input, is_reply) = is_reply(input)?;
    let (input, _type) = _type(input)?;
    let (input, id) = id(input)?;
    let (input, error_code) = error_code(input)?;
    let (input, total_size) = total_size(input)?;
    let (input, data_size) = data_size(input)?;

    let header = TransactionHeader {
        flags,
        is_reply,
        _type,
        id,
        error_code,
        total_size,
        data_size,
    };

    Ok((input, header))
}
