use super::{
    ErrorCode,
    ProtocolError,
    TransactionType,
    TransactionField,
    HotlineProtocol,
};

#[derive(Debug, Clone, Copy)]
pub struct Flags(i8);
#[derive(Debug, Clone, Copy)]
pub struct IsReply(i8);
#[derive(Debug, Clone, Copy)]
pub struct Type(i16);
#[derive(Debug, Clone, Copy)]
pub struct Id(i32);
#[derive(Debug, Clone, Copy)]
pub struct TotalSize(i32);
#[derive(Debug, Clone, Copy)]
pub struct DataSize(i32);

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
#[derive(Debug, Clone, Copy)]
struct FieldSize(i16);
#[derive(Debug, Clone, Copy)]
struct ParameterCount(i16);

#[derive(Debug, Clone)]
struct Parameter {
    field_id: FieldId,
    field_data: Vec<u8>,
}

impl Parameter {
    pub fn field_matches(&self, field: TransactionField) -> bool {
        self.field_id.0 == field as i16
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
}

#[derive(Debug, Clone)]
pub struct TransactionBody {
    parameters: Vec<Parameter>,
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
}
