use super::{
    ErrorCode,
    ProtocolError,
    TransactionType,
    TransactionField,
    HotlineProtocol,
    DekuHotlineProtocol,
    BIResult,
    be_i8,
    be_i16,
    be_i32,
    be_i64,
};

use derive_more::{From, Into};
use encoding_rs::MACINTOSH;
use deku::prelude::*;

#[derive(Debug, Clone, Copy, From, Into, DekuRead, DekuWrite)]
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

#[derive(Debug, Clone, Copy, Default, From, Into, DekuRead, DekuWrite)]
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

impl From<IsReply> for bool {
    fn from(val: IsReply) -> Self {
        val.0 == 1
    }
}

#[derive(Debug, Clone, Copy, Default, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct Type(i16);

#[derive(Debug, Clone, Copy, Default, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct Id(i32);

#[derive(Debug, Clone, Copy, Default, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct TotalSize(i32);

#[derive(Debug, Clone, Copy, Default, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct DataSize(i32);

impl From<usize> for DataSize {
    fn from(size: usize) -> Self {
        (size as i32).into()
    }
}

#[derive(Debug, Clone, Copy, DekuRead, DekuWrite)]
pub struct TransactionHeader {
    pub flags: Flags,
    pub is_reply: IsReply,
    pub type_: Type,
    pub id: Id,
    pub error_code: ErrorCode,
    pub total_size: TotalSize,
    pub data_size: DataSize,
}

impl TransactionHeader {
    pub fn transaction_type(&self) -> Result<TransactionType, ProtocolError> {
        let Type(type_id) = self.type_;
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
            type_: TransactionType::Reply.into(),
            id: request.id,
            is_reply: IsReply::reply(),
            ..self
        }
    }
    pub fn update_sizes(&mut self, size: usize) {
        self.data_size = DataSize(size as i32);
        self.total_size = TotalSize(size as i32);
    }
}

impl Default for TransactionHeader {
    fn default() -> Self {
        Self {
            type_: TransactionType::Error.into(),
            id: 0.into(),
            error_code: ErrorCode::ok(),
            is_reply: IsReply::request(),
            flags: Flags::default(),
            total_size: 0.into(),
            data_size: 0.into(),
        }
    }
}

impl From<TransactionType> for TransactionHeader {
    fn from(_type: TransactionType) -> Self {
        Self {
            type_: _type.into(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Copy, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
pub struct FieldId(i16);

#[derive(Debug, Clone, Copy, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
struct FieldSize(i16);

#[derive(Debug, Clone, Copy, From, Into, DekuRead, DekuWrite)]
#[deku(endian = "big")]
struct ParameterCount(i16);

#[derive(Debug, Clone, DekuRead, DekuWrite)]
pub struct Parameter {
    pub field_id: FieldId,
    #[deku(endian = "big", update = "self.field_data.len()")]
    pub field_size: i16,
    #[deku(count = "field_size")]
    pub field_data: Vec<u8>,
}

impl Parameter {
    pub fn new<F: Into<FieldId>>(field_id: F, field_data: Vec<u8>) -> Self {
        Self {
            field_id: field_id.into(),
            field_size: field_data.len() as i16,
            field_data,
        }
    }
    pub fn new_i16<F: Into<FieldId>>(field_id: F, int: i16) -> Self {
        Self::new(field_id, int.to_be_bytes().to_vec())
    }
    pub fn new_i32<F: Into<FieldId>>(field_id: F, int: i32) -> Self {
        Self::new(field_id, int.to_be_bytes().to_vec())
    }
    pub fn new_int<F, I>(field_id: F, int: I) -> Self
        where F: Into<FieldId>,
              I: Into<IntParameter> {
        let field_id = field_id.into();
        let param = int.into();
        let field_data: Vec<u8> = param.into();
        let field_size = field_data.len() as i16;
        Self {
            field_id,
            field_size,
            field_data,
        }
    }
    pub fn new_data(data: Vec<u8>) -> Self {
        Self::new(TransactionField::Data, data)
    }
    pub fn new_error<S: AsRef<str>>(message: S) -> Self {
        let message = message.as_ref();
        let (message, _, _) = MACINTOSH.encode(message);
        Self::new(TransactionField::ErrorText, message.to_vec())
    }
    pub fn field_matches(&self, field: TransactionField) -> bool {
        self.field_id.0 == field as i16
    }
    pub fn take(self) -> Vec<u8> {
        self.field_data
    }
    pub fn int(&self) -> Option<IntParameter> {
        self.into()
    }
    pub fn compute_length(&self) -> usize {
        2 + 2 + self.field_data.len()
    }
}

impl std::borrow::Borrow<[u8]> for Parameter {
    fn borrow(&self) -> &[u8] {
        &self.field_data
    }
}

#[derive(Debug, Clone, Copy, From, Into)]
#[from(i8, i16, i32)]
pub struct IntParameter(i64);

impl IntParameter {
    pub fn from_i8(data: &[u8]) -> Option<i64> {
        if let Ok((b"", i)) = be_i8::<_, nom::error::Error<_>>(data) {
            Some(i as i64)
        } else {
            None
        }
    }
    pub fn from_i16(data: &[u8]) -> Option<i64> {
        if let Ok((b"", i)) = be_i16::<_, nom::error::Error<_>>(data) {
            Some(i as i64)
        } else {
            None
        }
    }
    pub fn from_i32(data: &[u8]) -> Option<i64> {
        if let Ok((b"", i)) = be_i32::<_, nom::error::Error<_>>(data) {
            Some(i as i64)
        } else {
            None
        }
    }
    pub fn from_i64(data: &[u8]) -> Option<i64> {
        if let Ok((b"", i)) = be_i64::<_, nom::error::Error<_>>(data) {
            Some(i)
        } else {
            None
        }
    }
    pub fn i8(&self) -> Option<i8> {
        let Self(int) = self;
        i8::try_from(*int).ok()
    }
    pub fn i16(&self) -> Option<i16> {
        let Self(int) = self;
        i16::try_from(*int).ok()
    }
    pub fn i32(&self) -> Option<i32> {
        let Self(int) = self;
        i32::try_from(*int).ok()
    }
}

impl From<&Parameter> for Option<IntParameter> {
    fn from(p: &Parameter) -> Self {
        let data = p.field_data.as_slice();
        let value = match data.len() {
            1 => IntParameter::from_i8(data),
            2 => IntParameter::from_i16(data),
            4 => IntParameter::from_i32(data),
            8 => IntParameter::from_i64(data),
            _ => None,
        };
        value.map(IntParameter)
    }
}

impl From<IntParameter> for Vec<u8> {
    fn from(val: IntParameter) -> Self {
        let IntParameter(int) = val;
        if int < (i16::MIN as i64) {
            int.to_be_bytes().to_vec()
        } else if int < (i8::MIN as i64) {
            (int as i16).to_be_bytes().to_vec()
        } else if int <= (i8::MAX as i64) {
            (int as i8).to_be_bytes().to_vec()
        } else if int <= (i16::MAX as i64) {
            (int as i16).to_be_bytes().to_vec()
        } else {
            int.to_be_bytes().to_vec()
        }
    }
}

#[derive(Debug, Clone, Default, DekuRead, DekuWrite)]
pub struct TransactionBody {
    #[deku(endian = "big", update = "self.parameters.len()")]
    parameter_count: i16,
    #[deku(count = "parameter_count")]
    pub parameters: Vec<Parameter>,
}

impl TransactionBody {
    pub fn borrow_field(&self, field: TransactionField) -> Option<&Parameter> {
        let Self { parameters, .. } = self;
        parameters.iter()
            .find(|p| p.field_matches(field))
    }
    pub fn borrow_fields(&self, field: TransactionField) -> Vec<&Parameter> {
        let Self { parameters, .. } = self;
        parameters.iter()
            .filter(|p| p.field_matches(field))
            .collect()
    }
    pub fn require_field(&self, field: TransactionField) -> Result<&Parameter, ProtocolError> {
        self.borrow_field(field)
            .ok_or(ProtocolError::MissingField(field))
    }
    pub fn compute_length(&self) -> usize {
        2 + self.parameters.iter().map(Parameter::compute_length).sum::<usize>()
    }
}

impl FromIterator<Parameter> for TransactionBody {
    fn from_iter<I: IntoIterator<Item=Parameter>>(iter: I) -> Self {
        Vec::from_iter(iter).into()
    }
}

impl From<Vec<Parameter>> for TransactionBody {
    fn from(parameters: Vec<Parameter>) -> Self {
        Self {
            parameter_count: parameters.len() as i16,
            parameters,
        }
    }
}

#[derive(Debug, Clone, DekuRead, DekuWrite)]
pub struct TransactionFrame {
    #[deku(update = "{
        self.header.update_sizes(self.body.compute_length());
        self.header
    }")]
    pub header: TransactionHeader,
    pub body: TransactionBody,
}

impl TransactionFrame {
    pub fn empty<H: Into<TransactionHeader>>(header: H) -> Self {
        Self {
            header: header.into(),
            body: Default::default(),
        }
    }
    pub fn new<H: Into<TransactionHeader>, B: Into<TransactionBody>>(
        header: H,
        body: B,
    ) -> Self {
        Self {
            header: header.into(),
            body: body.into(),
        }
    }
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

impl From<(TransactionHeader, TransactionBody)> for TransactionFrame {
    fn from(val: (TransactionHeader, TransactionBody)) -> Self {
        let (header, body) = val;
        Self { header, body }
    }
}

pub trait IntoFrameExt {
    fn framed(self) -> TransactionFrame;
    fn reply_to(self, request: &TransactionHeader) -> TransactionFrame;
}

impl <F: Into<TransactionFrame>> IntoFrameExt for F {
    fn framed(self) -> TransactionFrame {
        self.into()
    }
    fn reply_to(self, request: &TransactionHeader) -> TransactionFrame {
        self.framed().reply_to(request)
    }
}

impl DekuHotlineProtocol for TransactionHeader {}
impl DekuHotlineProtocol for TransactionBody {}

impl HotlineProtocol for TransactionFrame {
    fn into_bytes(mut self) -> Vec<u8> {
        self.update().unwrap();
        self.to_bytes().unwrap()
    }
    fn from_bytes(bytes: &[u8]) -> BIResult<Self> {
        let ((bytes, _bits), value) = <Self as DekuContainerRead>::from_bytes((bytes, 0)).unwrap();
        Ok((bytes, value))
    }
}
