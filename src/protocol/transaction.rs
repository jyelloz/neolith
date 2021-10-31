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
    take,
    multi,
};

#[derive(Debug, Clone, Copy)]
pub struct Flags(i8);

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
    flags: Flags,
    is_reply: IsReply,
    _type: Type,
    id: Id,
    error_code: ErrorCode,
    total_size: TotalSize,
    data_size: DataSize,
}

impl TransactionHeader {
    pub fn transaction_type(&self) -> Result<TransactionType, ProtocolError> {
        let Self { _type, ..  } = self;
        let Type(type_id) = *_type;
        TransactionType::try_from(type_id)
            .map_err(|_| ProtocolError::UnsupportedTransaction(*_type))
    }
    pub fn require_transaction_type(self, expected: TransactionType) -> Result<Self, ProtocolError> {
        let _type = self.transaction_type()?;
        if _type == expected {
            Ok(self)
        } else {
            let expected = Type(expected.into());
            let _type = Type(_type.into());
            Err(ProtocolError::UnexpectedTransaction{
                expected,
                encountered: _type,
            })
        }
    }
    pub fn body_len(&self) -> usize {
        self.data_size.0 as usize
    }
}

impl HotlineProtocol for TransactionHeader {
    fn into_bytes(self) -> Vec<u8> {
        let flags = self.flags.0.to_be_bytes();
        let is_reply = self.is_reply.0.to_be_bytes();
        let _type = self._type.0.to_be_bytes();
        let id = self.id.0.to_be_bytes();
        let error_code = self.error_code.0.to_be_bytes();
        let total_size = self.total_size.0.to_be_bytes();
        let data_size = self.data_size.0.to_be_bytes();
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
}

#[derive(Debug, Clone, Copy)]
struct FieldId(i16);

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
struct Parameter {
    field_id: FieldId,
    field_data: Vec<u8>,
}

impl Parameter {
    pub fn field_matches(&self, field: TransactionField) -> bool {
        self.field_id.0 == field as i16
    }
    fn field_data(bytes: &[u8], size: usize) -> BIResult<Vec<u8>> {
        let (bytes, data) = take(size)(bytes)?;
        Ok((bytes, data.to_vec()))
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

#[derive(Debug, Clone)]
pub struct TransactionBody {
    parameters: Vec<Parameter>,
}

impl TransactionBody {
    fn parameter_count(bytes: &[u8]) -> BIResult<usize> {
        let (bytes, count) = be_i16(bytes)?;
        Ok((bytes, count as usize))
    }
    fn parameter_list(input: &[u8], count: usize) -> BIResult<Vec<Parameter>> {
        multi::count(Parameter::from_bytes, count)(input)
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
        let (input, count) = Self::parameter_count(bytes)?;
        let (input, parameters) = Self::parameter_list(input, count)?;
        let body = TransactionBody { parameters };
        Ok((input, body))
    }
}
