use deku::prelude::*;
use neolith::protocol as proto;
use std::io::{Cursor, prelude::*};
use bytes::Buf;

#[derive(Default)]
pub struct HeaderReader(Cursor<[u8; Self::SIZE]>);
impl HeaderReader {
    const SIZE: usize = 20;
    pub fn parse(&mut self, buf: &[u8]) -> (usize, Option<proto::TransactionHeader>) {
        let Self(cursor) = self;
        let need = cursor.remaining();
        let len = need.min(buf.len());
        let buf = &buf[..len];
        cursor.write_all(buf).expect("failed to copy to cursor");
        if cursor.remaining() == 0 {
            let (_, header) = proto::TransactionHeader::from_bytes((cursor.get_ref(), 0)).expect("failed to read header");
            (len, Some(header))
        } else {
            (len, None)
        }
    }
}

#[derive(Default)]
pub struct ParameterCountReader(Cursor<[u8; Self::SIZE]>);
impl ParameterCountReader {
    const SIZE: usize = size_of::<u16>();
    pub fn parse(&mut self, buf: &[u8]) -> (usize, Option<usize>) {
        let Self(cursor) = self;
        let need = cursor.remaining();
        let len = need.min(buf.len());
        let buf = &buf[..len];
        cursor.write_all(buf).expect("failed to copy to cursor");
        if cursor.remaining() == 0 {
            let val = u16::from_be_bytes(*cursor.get_ref());
            (len, Some(val as usize))
        } else {
            (len, None)
        }
    }
}

#[derive(Default)]
pub struct FieldIdReader(Cursor<[u8; Self::SIZE]>);
impl FieldIdReader {
    const SIZE: usize = size_of::<i16>();
    pub fn parse(&mut self, buf: &[u8]) -> (usize, Option<proto::FieldId>) {
        let Self(cursor) = self;
        let need = cursor.remaining();
        let len = need.min(buf.len());
        let buf = &buf[..len];
        cursor.write_all(buf).expect("failed to copy to cursor");
        if cursor.remaining() == 0 {
            let (_, id) = proto::FieldId::from_bytes((cursor.get_ref(), 0)).expect("failed to read field id");
            (len, Some(id))
        } else {
            (len, None)
        }
    }
}

#[derive(Default)]
pub struct FieldSizeReader(Cursor<[u8; Self::SIZE]>);
impl FieldSizeReader {
    const SIZE: usize = size_of::<u16>();
    pub fn parse(&mut self, buf: &[u8]) -> (usize, Option<usize>) {
        let Self(cursor) = self;
        let need = cursor.remaining();
        let len = need.min(buf.len());
        let buf = &buf[..len];
        cursor.write_all(buf).expect("failed to copy to cursor");
        if cursor.remaining() == 0 {
            let val = u16::from_be_bytes(*cursor.get_ref());
            (len, Some(val as usize))
        } else {
            (len, None)
        }
    }
}

pub struct FieldDataReader(Cursor<Box<[u8]>>);
impl FieldDataReader {
    pub fn new(data_size: usize) -> Self {
        Self (
             Cursor::new(vec![0u8; data_size].into_boxed_slice()),
        )
    }
    pub fn parse(&mut self, buf: &[u8]) -> (usize, Option<Vec<u8>>) {
        let Self(pending_data) = self;
        let read_len = buf.len().min(pending_data.remaining());
        let buf = &buf[..read_len];
        pending_data.write_all(buf).unwrap();
        if pending_data.remaining() == 0 {
            let param = pending_data.get_ref().to_vec();
            (read_len, Some(param))
        } else {
            (read_len, None)
        }
    }
}

enum TransactionParseState {
    Header(HeaderReader),
    ParameterCount(ParameterCountReader),
    ParameterFieldId(FieldIdReader),
    ParameterFieldSize(FieldSizeReader),
    ParameterFieldData(FieldDataReader),
}

impl Default for TransactionParseState {
    fn default() -> Self {
        Self::Header(HeaderReader::default())
    }
}

struct TransactionParseResponse(usize, Option<proto::TransactionFrame>);

struct TransactionParser {
    state: TransactionParseState,
    hdr: Option<proto::TransactionHeader>,
    param_count: usize,
    current_param: proto::Parameter,
    params: Vec<proto::Parameter>,
}

impl Default for TransactionParser {
    fn default() -> Self {
        Self {
            state: TransactionParseState::default(),
            hdr: None,
            param_count: 0,
            params: vec![],
            current_param: proto::Parameter {
                field_id: 0.into(),
                field_size: 0i16.into(),
                field_data: vec![],
            },
        }
    }
}

impl TransactionParser {
    pub fn parse(&mut self, data: &[u8]) -> TransactionParseResponse {
        let len = match &mut self.state {
            TransactionParseState::Header(rdr) => {
                let (len, header) = rdr.parse(&data);
                let Some(header) = header else {
                    eprintln!("not enough data for header");
                    return TransactionParseResponse(0, None)
                };
                if i32::from(header.data_size) <= len as i32 {
                    self.state = TransactionParseState::Header(HeaderReader::default());
                    let transaction = proto::TransactionFrame::empty(header);
                    return TransactionParseResponse(len, Some(transaction))
                }
                self.state = TransactionParseState::ParameterCount(ParameterCountReader::default());
                len
            }
            TransactionParseState::ParameterCount(rdr) => {
                let (len, count) = rdr.parse(data);
                let Some(count) = count else {
                    return TransactionParseResponse(len, None)
                };
                if count == 0 {
                    self.state = TransactionParseState::Header(HeaderReader::default());
                    let header = self.hdr.take().unwrap();
                    let transaction = proto::TransactionFrame::empty(header);
                    return TransactionParseResponse(len, Some(transaction))
                }
                self.param_count = count;
                self.state = TransactionParseState::ParameterFieldId(FieldIdReader::default());
                len
            },
            TransactionParseState::ParameterFieldId(rdr) => {
                let (len, id) = rdr.parse(data);
                let Some(id) = id else {
                    eprintln!("not enough data for field id");
                    return TransactionParseResponse(len, None)
                };
                self.current_param.field_id = id;
                self.state = TransactionParseState::ParameterFieldSize(FieldSizeReader::default());
                len
            },
            TransactionParseState::ParameterFieldSize(rdr) => {
                let (len, size) = rdr.parse(data);
                let Some(size) = size else {
                    return TransactionParseResponse(len, None)
                };
                self.current_param.field_size = size as i16;
                self.state = TransactionParseState::ParameterFieldData(FieldDataReader::new(size));
                len
            },
            TransactionParseState::ParameterFieldData(rdr) => {
                let (len, field_data) = rdr.parse(data);
                let Some(field_data) = field_data else {
                    eprintln!("not enough data for field data");
                    return TransactionParseResponse(len, None)
                };
                self.current_param.field_data = field_data;
                self.params.push(self.current_param.clone());
                self.param_count -= 1;
                if self.param_count == 0 {
                    let header = self.hdr.take().unwrap();
                    let transaction = proto::TransactionFrame::new(header, self.params.clone());
                    eprintln!("transaction {transaction:?}");
                    self.params = vec![];
                    self.state = TransactionParseState::Header(HeaderReader::default());
                    return TransactionParseResponse(len, Some(transaction))
                }
                self.state = TransactionParseState::ParameterFieldId(FieldIdReader::default());
                len
            },
        };
        TransactionParseResponse(len, None)
    }
}

fn main() -> anyhow::Result<()> {
    let data = include_bytes!("../packets/edit-user-error.bin");
    let data = [&data[..], &data[..], &[0u8; 12], &123u32.to_be_bytes(), &456u32.to_be_bytes(), &[0u8; 2]].concat();
    let mut data = &data[..];
    let mut state = TransactionParseState::Header(HeaderReader::default());
    let mut hdr = None::<proto::TransactionHeader>;
    let mut param_count = 0;
    let mut params = vec![];
    let mut current_param = proto::Parameter {
        field_id: 0.into(),
        field_size: 0i16.into(),
        field_data: vec![],
    };
    while data.len() > 0 {
        eprintln!("{} bytes to go", data.len());
        match &mut state {
            TransactionParseState::Header(rdr) => {
                let (len, header) = rdr.parse(&data);
                data = &data[len..];
                let Some(header) = header else {
                    eprintln!("not enough data for header");
                    continue;
                };
                hdr.replace(header);
                if i32::from(header.data_size) <= len as i32 {
                    eprintln!("no transaction body");
                    state = TransactionParseState::Header(HeaderReader::default());
                    let header = hdr.take().unwrap();
                    let transaction = proto::TransactionFrame::empty(header);
                    eprintln!("transaction {transaction:?}");
                    continue;
                }
                state = TransactionParseState::ParameterCount(ParameterCountReader::default());
            }
            TransactionParseState::ParameterCount(rdr) => {
                let (len, count) = rdr.parse(data);
                data = &data[len..];
                let Some(count) = count else {
                    eprintln!("not enough data for parameter count");
                    continue;
                };
                if count == 0 {
                    state = TransactionParseState::Header(HeaderReader::default());
                    let header = hdr.take().unwrap();
                    let transaction = proto::TransactionFrame::empty(header);
                    eprintln!("transaction {transaction:?}");
                } else {
                    param_count = count;
                    state = TransactionParseState::ParameterFieldId(FieldIdReader::default());
                }
            },
            TransactionParseState::ParameterFieldId(rdr) => {
                let (len, id) = rdr.parse(data);
                data = &data[len..];
                let Some(id) = id else {
                    eprintln!("not enough data for field id");
                    continue;
                };
                current_param.field_id = id;
                state = TransactionParseState::ParameterFieldSize(FieldSizeReader::default());
            },
            TransactionParseState::ParameterFieldSize(rdr) => {
                let (len, size) = rdr.parse(data);
                data = &data[len..];
                let Some(size) = size else {
                    eprintln!("not enough data for field size");
                    continue;
                };
                current_param.field_size = size as i16;
                state = TransactionParseState::ParameterFieldData(FieldDataReader::new(size));
            },
            TransactionParseState::ParameterFieldData(rdr) => {
                let (len, field_data) = rdr.parse(data);
                data = &data[len..];
                let Some(field_data) = field_data else {
                    eprintln!("not enough data for field data");
                    continue;
                };
                current_param.field_data = field_data;
                params.push(current_param.clone());
                param_count -= 1;
                if param_count == 0 {
                    let header = hdr.take().unwrap();
                    let transaction = proto::TransactionFrame::new(header, params.clone());
                    eprintln!("transaction {transaction:?}");
                    params = vec![];
                    state = TransactionParseState::Header(HeaderReader::default());
                } else {
                    state = TransactionParseState::ParameterFieldId(FieldIdReader::default());
                }
            },
        }
    }
    Ok(())
}
