use nom::{
    self,
    IResult,
    multi,
    bytes::{
        self,
        streaming::take,
    },
    number::streaming::{
        be_i32,
        be_i16,
        be_i8,
    },
};

mod transaction_type;
mod transaction_field;

use transaction_field::TransactionField;

pub(crate) enum Transaction {
    Login(LoginRequest),
    AgreedToTerms,
    KeepAlive,
    ClientDisconnect,
}

type BIResult<'a, T> = IResult<&'a [u8], T>;

#[derive(Debug)]
struct ClientHandshakeRequest {
    sub_protocol_id: SubProtocolId,
    version: Version,
    sub_version: SubVersion,
}
#[derive(Debug)]
struct ServerHandshakeReply {
    error_code: ErrorCode,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ProtocolId(i32);
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SubProtocolId(i32);
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version(i16);
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SubVersion(i16);
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ErrorCode(i32);

#[derive(Debug)]
struct TransactionHeader {
    flags: Flags,
    is_reply: IsReply,
    _type: Type,
    id: Id,
    error_code: ErrorCode,
    total_size: TotalSize,
    data_size: DataSize,
}

#[derive(Debug)]
struct Flags(i8);
#[derive(Debug)]
struct IsReply(i8);
#[derive(Debug)]
struct Type(i16);
#[derive(Debug)]
struct Id(i32);
#[derive(Debug)]
struct TotalSize(i32);
#[derive(Debug)]
struct DataSize(i32);

#[derive(Debug)]
struct Parameter {
    field_id: FieldId,
    field_size: FieldSize,
    field_data: Vec<u8>,
}

impl Parameter {
    pub fn field_matches(&self, field: TransactionField) -> bool {
        self.field_id.0 == field as i16
    }
}

#[derive(Debug)]
struct FieldId(i16);
#[derive(Debug)]
struct FieldSize(i16);
#[derive(Debug)]
struct ParameterCount(i16);

#[derive(Debug)]
struct TransactionBody {
    parameters: Vec<Parameter>,
}

fn sub_protocol_id(input: &[u8]) -> BIResult<SubProtocolId> {
    let (input, id) = be_i32(input)?;
    Ok((input, SubProtocolId(id)))
}

fn version(input: &[u8]) -> BIResult<Version> {
    let (input, version) = be_i16(input)?;
    Ok((input, Version(version)))
}

fn sub_version(input: &[u8]) -> BIResult<SubVersion> {
    let (input, sub_version) = be_i16(input)?;
    Ok((input, SubVersion(sub_version)))
}

fn client_handshake_request(input: &[u8]) -> BIResult<ClientHandshakeRequest> {
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

fn error_code(input: &[u8]) -> BIResult<ErrorCode> {
    be_i32(input).map(
        |(input, code)| (input, ErrorCode(code))
    )
}

fn server_handshake_reply(input: &[u8]) -> BIResult<ServerHandshakeReply> {
    let (input, _) = bytes::streaming::tag(r"TRTP")(input)?;
    let (input, error_code) = error_code(input)?;
    Ok((input, ServerHandshakeReply { error_code }))
}

fn flags(input: &[u8]) -> BIResult<Flags> {
    be_i8(input).map(|(input, flags)| (input, Flags(flags)))
}

fn is_reply(input: &[u8]) -> BIResult<IsReply> {
    be_i8(input).map(|(input, is_reply)| (input, IsReply(is_reply)))
}

fn id(input: &[u8]) -> BIResult<Id> {
    be_i32(input).map(|(input, id)| (input, Id(id)))
}

fn _type(input: &[u8]) -> BIResult<Type> {
    be_i16(input).map(|(input, _type)| (input, Type(_type)))
}

fn total_size(input: &[u8]) -> BIResult<TotalSize> {
    be_i32(input).map(|(input, size)| (input, TotalSize(size)))
}

fn data_size(input: &[u8]) -> BIResult<DataSize> {
    be_i32(input).map(|(input, size)| (input, DataSize(size)))
}

fn transaction_header(input: &[u8]) -> BIResult<TransactionHeader> {

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

fn field_id(input: &[u8]) -> BIResult<FieldId> {
    be_i16(input).map(|(input, id)| (input, FieldId(id)))
}

fn field_size(input: &[u8]) -> BIResult<FieldSize> {
    be_i16(input).map(|(input, size)| (input, FieldSize(size)))
}

fn field_data(input: &[u8], size: usize) -> BIResult<Vec<u8>> {
    let (input, data) = take(size)(input)?;
    dbg!(data);
    Ok((input, data.to_vec()))
}

fn parameter(input: &[u8]) -> BIResult<Parameter> {
    let (input, field_id) = field_id(input)?;
    let (input, field_size) = field_size(input)?;
    let (input, field_data) = field_data(input, field_size.0 as usize)?;
    let parameter = Parameter {
        field_id,
        field_size,
        field_data,
    };
    Ok((input, parameter))
}
fn parameter_list(input: &[u8], count: usize) -> BIResult<Vec<Parameter>> {
    multi::count(parameter, count)(input)
}

fn transaction_body(input: &[u8]) -> BIResult<TransactionBody> {
    let (input, parameter_count) = be_i16(input)?;
    let (input, parameters) = parameter_list(input, parameter_count as usize)?;
    let body = TransactionBody { parameters };
    Ok((input, body))
}

#[derive(Debug)]
enum ProtocolError {
    MissingField(TransactionField),
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LoginRequest {
    pub username: Username,
    pub nickname: Nickname,
    pub password: Option<Password>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Username(Vec<u8>);
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Nickname(Vec<u8>);
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Password(Vec<u8>);

fn invert_credential(data: &[u8]) -> Vec<u8> {
    data.iter()
        .map(|b| !b)
        .collect()
}

trait Credential {
    fn deobfuscate(&self) -> Vec<u8>;
}

impl Username {
    pub fn new(username: Vec<u8>) -> Self {
        Self(username)
    }
    pub fn from_cleartext(clear: &[u8]) -> Self {
        Self(invert_credential(clear))
    }
    pub fn raw_data(&self) -> &[u8] {
        &self.0
    }
    pub fn take(self) -> Vec<u8> {
        self.0
    }
}

impl Nickname {
    pub fn new(nickname: Vec<u8>) -> Self {
        Self(nickname)
    }
}

impl Credential for Username {
    fn deobfuscate(&self) -> Vec<u8> {
        invert_credential(&self.0)
    }
}

impl Password {
    pub fn new(password: Vec<u8>) -> Self {
        Self(password)
    }
    pub fn from_cleartext(clear: &[u8]) -> Self {
        Self(invert_credential(clear))
    }
    pub fn raw_data(&self) -> &[u8] {
        &self.0
    }
    pub fn take(self) -> Vec<u8> {
        self.0
    }
}

impl Credential for Password {
    fn deobfuscate(&self) -> Vec<u8> {
        invert_credential(&self.0)
    }
}

impl TryFrom<TransactionBody> for LoginRequest {
    type Error = ProtocolError;
    fn try_from(body: TransactionBody) -> Result<Self, Self::Error> {

        let TransactionBody { parameters, .. } = body;

        let username = parameters.iter()
            .filter(|p| p.field_matches(TransactionField::Username))
            .map(|p| p.field_data.clone())
            .map(Username::new)
            .next()
            .ok_or(ProtocolError::MissingField(TransactionField::Username))?;

        let nickname = parameters.iter()
            .filter(|p| p.field_matches(TransactionField::Nickname))
            .map(|p| p.field_data.clone())
            .map(Nickname::new)
            .next()
            .ok_or(ProtocolError::MissingField(TransactionField::Nickname))?;

        let password = parameters.iter()
            .filter(|p| p.field_matches(TransactionField::Password))
            .map(|p| p.field_data.clone())
            .map(Password::new)
            .next();

        Ok(Self { username, nickname, password })
    }
}

impl From<TransactionField> for FieldId {
    fn from(field: TransactionField) -> Self {
        Self(field as i16)
    }
}

impl From<&[u8]> for FieldSize {
    fn from(data: &[u8]) -> Self {
        Self(data.len() as i16)
    }
}

impl Into<Parameter> for Username {
    fn into(self) -> Parameter {
        let Self(username) = self;
        Parameter {
            field_id: TransactionField::Username.into(),
            field_size: FieldSize::from(username.as_ref()),
            field_data: username,
        }
    }
}

impl Into<Parameter> for Nickname {
    fn into(self) -> Parameter {
        let Self(nickname) = self;
        Parameter {
            field_id: TransactionField::Nickname.into(),
            field_size: FieldSize::from(nickname.as_ref()),
            field_data: nickname,
        }
    }
}

impl Into<Parameter> for Password {
    fn into(self) -> Parameter {
        let Self(password) = self;
        Parameter {
            field_id: TransactionField::Password.into(),
            field_size: FieldSize::from(password.as_ref()),
            field_data: password,
        }
    }
}

impl Into<TransactionBody> for LoginRequest {
    fn into(self) -> TransactionBody {

        let Self { username, nickname, password } = self;

        let username = username.into();
        let nickname = nickname.into();
        let password = password.map(Password::into);

        let parameters = if let Some(password) = password {
            vec![username, nickname, password]
        } else {
            vec![username, nickname]
        };

        TransactionBody { parameters }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    static AUTHENTICATED_LOGIN: &'static [u8] = &[
        0x00, 0x00, 0x00, 0x6b,
        0x00, 0x00, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x28,
        0x00, 0x00, 0x00, 0x28,
        0x00, 0x04, 0x00, 0x69,
        0x00, 0x07, 0x95, 0x86,
        0x9a, 0x93, 0x93, 0x90,
        0x85, 0x00, 0x6a, 0x00,
        0x06, 0xce, 0xcd, 0xcc,
        0xcb, 0xca, 0xc9, 0x00,
        0x66, 0x00, 0x07, 0x6a,
        0x79, 0x65, 0x6c, 0x6c,
        0x6f, 0x7a, 0x00, 0x68,
        0x00, 0x02, 0x00, 0x91,
    ];

    static CLIENT_HANDSHAKE: &'static [u8] = &[
        0x54, 0x52, 0x54, 0x50,
        0x48, 0x4f, 0x54, 0x4c,
        0x00, 0x01, 0x00, 0x02,
    ];

    static SERVER_HANDSHAKE: &'static [u8] = &[
        0x54, 0x52, 0x54, 0x50,
        0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn parse_client_handshake() {

        let (tail, _handshake) = client_handshake_request(CLIENT_HANDSHAKE)
            .expect("could not parse client handshake");

        assert!(tail.is_empty());

    }

    #[test]
    fn parse_server_handshake() {

        let (tail, handshake) = server_handshake_reply(SERVER_HANDSHAKE)
            .expect("could not parse server handshake");

        assert!(tail.is_empty());

        assert_eq!(
            handshake.error_code,
            ErrorCode(0),
        );

    }

    #[test]
    fn parse_authenticated_login() {

        let (tail, _header) = transaction_header(AUTHENTICATED_LOGIN)
            .expect("could not parse transaction header");

        let (tail, login) = transaction_body(tail)
            .expect("could not parse valid login packet");

        assert!(tail.is_empty());

        let login = LoginRequest::try_from(login)
            .expect("could not view transaction as login request");

        assert_eq!(
            login,
            LoginRequest {
                username: Username::from_cleartext(b"jyelloz"),
                nickname: Nickname::new(b"jyelloz".clone().into()),
                password: Some(Password::from_cleartext(b"123456")),
            },
        );

    }

}
