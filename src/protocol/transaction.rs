use super::{
    ErrorCode,
    ProtocolError,
    TransactionType,
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
