use super::{
    ErrorCode,
    ProtocolError,
    TransactionType,
    TransactionField,
    HotlineProtocol,
    BIResult,
    be_i8,
    be_i16,
    be_i32,
    be_u8,
    be_u16,
    be_u32,
    take,
    multi,
};

#[derive(Debug, Clone, Copy)]
pub struct Flags(i8);

impl Flags {
    pub fn none() -> Self {
        Self(0)
    }
}

impl Default for Flags {
    fn default() -> Self {
        Self::none()
    }
}

impl HotlineProtocol for Flags {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i8(bytes)?;
        Ok((bytes, Self(value)))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct IsReply(i8);

impl IsReply {
    pub fn reply() -> Self {
        Self(1)
    }
    pub fn request() -> Self {
        Self(0)
    }
    pub fn is_reply(&self) -> bool {
        (*self).into()
    }
}

impl Into<bool> for IsReply {
    fn into(self) -> bool {
        self.0 == 1
    }
}

impl HotlineProtocol for IsReply {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i8(bytes)?;
        Ok((bytes, Self(value)))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Type(i16);

impl From<i16> for Type {
    fn from(int: i16) -> Self {
        Self(int)
    }
}
impl Into<i16> for Type {
    fn into(self) -> i16 {
        self.0
    }
}

impl HotlineProtocol for Type {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i16(bytes)?;
        Ok((bytes, Self(value)))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Id(i32);

impl From<i32> for Id {
    fn from(int: i32) -> Self {
        Self(int)
    }
}

impl Into<i32> for Id {
    fn into(self) -> i32 {
        self.0
    }
}

impl HotlineProtocol for Id {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i32(bytes)?;
        Ok((bytes, Self(value)))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TotalSize(i32);

impl From<i32> for TotalSize {
    fn from(int: i32) -> Self {
        Self(int)
    }
}

impl Into<i32> for TotalSize {
    fn into(self) -> i32 {
        self.0
    }
}

impl HotlineProtocol for TotalSize {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i32(bytes)?;
        Ok((bytes, Self(value)))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DataSize(i32);

impl From<i32> for DataSize {
    fn from(int: i32) -> Self {
        Self(int)
    }
}

impl Into<i32> for DataSize {
    fn into(self) -> i32 {
        self.0
    }
}

impl HotlineProtocol for DataSize {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i32(bytes)?;
        Ok((bytes, Self(value)))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TransactionHeader {
    pub flags: Flags,
    pub is_reply: IsReply,
    pub _type: Type,
    pub id: Id,
    pub error_code: ErrorCode,
    pub total_size: TotalSize,
    pub data_size: DataSize,
}

impl TransactionHeader {
    pub fn transaction_type(&self) -> Result<TransactionType, ProtocolError> {
        let Type(type_id) = self._type;
        TransactionType::try_from(type_id)
            .map_err(|_| ProtocolError::UnsupportedTransaction(type_id))
    }
    pub fn require_transaction_type(self, expected: TransactionType) -> Result<Self, ProtocolError> {
        let _type = self.transaction_type()?;
        if _type == expected {
            Ok(self)
        } else {
            let expected = expected.into();
            let _type = _type.into();
            Err(ProtocolError::UnexpectedTransaction {
                expected,
                encountered: _type,
            })
        }
    }
    pub fn body_len(&self) -> usize {
        self.data_size.0 as usize
    }
    pub fn reply_to(self, request: &TransactionHeader) -> Self {
        Self {
            _type: request._type,
            id: request.id,
            is_reply: IsReply::reply(),
            ..self
        }
    }
}

impl HotlineProtocol for TransactionHeader {
    fn into_bytes(self) -> Vec<u8> {
        let flags = self.flags.into_bytes();
        let is_reply = self.is_reply.into_bytes();
        let _type = self._type.into_bytes();
        let id = self.id.into_bytes();
        let error_code = self.error_code.into_bytes();
        let total_size = self.total_size.into_bytes();
        let data_size = self.data_size.into_bytes();
        [
            &flags[..],
            &is_reply[..],
            &_type[..],
            &id[..],
            &error_code[..],
            &total_size[..],
            &data_size[..],
        ].into_iter()
            .flat_map(|bytes| bytes.iter())
            .map(|b| *b)
            .collect()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, flags) = Flags::from_bytes(bytes)?;
        let (bytes, is_reply) = IsReply::from_bytes(bytes)?;
        let (bytes, _type) = Type::from_bytes(bytes)?;
        let (bytes, id) = Id::from_bytes(bytes)?;
        let (bytes, error_code) = ErrorCode::from_bytes(bytes)?;
        let (bytes, total_size) = TotalSize::from_bytes(bytes)?;
        let (bytes, data_size) = DataSize::from_bytes(bytes)?;
        let header = Self {
            flags,
            is_reply,
            _type,
            id,
            error_code,
            total_size,
            data_size,
        };
        Ok((bytes, header))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FieldId(i16);

impl From<i16> for FieldId {
    fn from(int: i16) -> Self {
        Self(int)
    }
}

impl Into<i16> for FieldId {
    fn into(self) -> i16 {
        self.0
    }
}

impl HotlineProtocol for FieldId {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i16(bytes)?;
        Ok((bytes, Self(value)))
    }
}

#[derive(Debug, Clone, Copy)]
struct FieldSize(i16);

impl From<&[u8]> for FieldSize {
    fn from(data: &[u8]) -> Self {
        Self(data.len() as i16)
    }
}

impl HotlineProtocol for FieldSize {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i16(bytes)?;
        Ok((bytes, Self(value)))
    }
}

#[derive(Debug, Clone, Copy)]
struct ParameterCount(i16);

impl HotlineProtocol for ParameterCount {
    fn into_bytes(self) -> Vec<u8> {
        let Self(value) = self;
        value.to_be_bytes().into()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, value) = be_i16(bytes)?;
        Ok((bytes, Self(value)))
    }
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub field_id: FieldId,
    field_data: Vec<u8>,
}

impl Parameter {
    pub fn new(field_id: FieldId, field_data: Vec<u8>) -> Self {
        Self { field_id, field_data }
    }
    pub fn field_matches(&self, field: TransactionField) -> bool {
        self.field_id.0 == field as i16
    }
    pub fn borrow(&self) -> &[u8] {
        &self.field_data
    }
    pub fn take(self) -> Vec<u8> {
        self.field_data
    }
    fn field_data(bytes: &[u8], size: usize) -> BIResult<Vec<u8>> {
        let (bytes, data) = take(size)(bytes)?;
        Ok((bytes, data.to_vec()))
    }
    pub fn int(self) -> Option<IntParameter> {
        (&self).into()
    }
}

impl HotlineProtocol for Parameter {
    fn into_bytes(self) -> Vec<u8> {
        let field_id = self.field_id.0.to_be_bytes();
        let field_size = (self.field_data.len() as i16).to_be_bytes();
        let field_data = self.field_data;
        [
            &field_id[..],
            &field_size[..],
            &field_data[..],
        ].into_iter()
            .flat_map(|bytes| bytes.iter())
            .map(|b| *b)
            .collect()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, field_id) = FieldId::from_bytes(bytes)?;
        let (bytes, field_size) = FieldSize::from_bytes(bytes)?;
        let field_size = field_size.0 as usize;
        let (bytes, field_data) = Self::field_data(bytes, field_size)?;
        let parameter = Parameter { field_id, field_data };
        Ok((bytes, parameter))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct IntParameter(u32);

impl IntParameter {
    fn from_byte(data: &[u8]) -> Option<u32> {
        if let Ok((b"", i)) = be_u8::<_, nom::error::Error<_>>(data) {
            Some(i as u32)
        } else {
            None
        }
    }
    fn from_ushort(data: &[u8]) -> Option<u32> {
        if let Ok((b"", i)) = be_u16::<_, nom::error::Error<_>>(data) {
            Some(i as u32)
        } else {
            None
        }
    }
    fn from_uint(data: &[u8]) -> Option<u32> {
        if let Ok((b"", i)) = be_u32::<_, nom::error::Error<_>>(data) {
            Some(i)
        } else {
            None
        }
    }
}

impl From<&Parameter> for Option<IntParameter> {
    fn from(p: &Parameter) -> Self {
        let data = p.field_data.as_slice();
        let value = match data.len() {
            1 => IntParameter::from_byte(data),
            2 => IntParameter::from_ushort(data),
            4 => IntParameter::from_uint(data),
            _ => None,
        };
        value.map(|v| IntParameter(v))
    }
}

impl Into<u32> for IntParameter {
    fn into(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct TransactionBody {
    pub parameters: Vec<Parameter>,
}

impl TransactionBody {
    fn parameter_count(bytes: &[u8]) -> BIResult<usize> {
        let (bytes, count) = be_i16(bytes)?;
        Ok((bytes, count as usize))
    }
    fn parameter_list(bytes: &[u8], count: usize) -> BIResult<Vec<Parameter>> {
        multi::count(Parameter::from_bytes, count)(bytes)
    }
}

impl HotlineProtocol for TransactionBody {
    fn into_bytes(self) -> Vec<u8> {
        let Self { parameters } = self;
        let parameter_count = (parameters.len() as i16).to_be_bytes();
        let parameters: Vec<u8> = parameters.into_iter()
            .map(HotlineProtocol::into_bytes)
            .flat_map(|bytes| bytes.into_iter())
            .collect();
        [
            &parameter_count[..],
            &parameters.as_slice()[..],
        ].into_iter()
            .flat_map(|bytes| bytes.iter())
            .map(|b| *b)
            .collect()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, count) = Self::parameter_count(bytes)?;
        let (bytes, parameters) = Self::parameter_list(bytes, count)?;
        let body = TransactionBody { parameters };
        Ok((bytes, body))
    }
}

#[derive(Debug, Clone)]
pub struct TransactionFrame {
    pub header: TransactionHeader,
    pub body: TransactionBody,
}

impl TransactionFrame {
    pub fn require_transaction_type(self, expected: TransactionType) -> Result<Self, ProtocolError> {
        self.header.require_transaction_type(expected)?;
        Ok(self)
    }
    pub fn reply_to(self, request: &TransactionHeader) -> Self {
        let Self { header, body } = self;
        Self {
            header: header.reply_to(request),
            body,
        }
    }
}

impl HotlineProtocol for TransactionFrame {
    fn into_bytes(self) -> Vec<u8> {
        let Self { header, body } = self;
        let body = body.into_bytes();
        let size = body.len() as i32;
        let header = TransactionHeader {
            total_size: TotalSize(size),
            data_size: DataSize(size),
            ..header
        }.into_bytes();
        [header, body].into_iter()
            .flat_map(|bytes| bytes.into_iter())
            .collect()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let (bytes, header) = TransactionHeader::from_bytes(bytes)?;
        let (bytes, body) = TransactionBody::from_bytes(bytes)?;
        let frame = Self { header, body };
        Ok((bytes, frame))
    }
}
