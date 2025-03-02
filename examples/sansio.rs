use bytes::Buf;
use deku::prelude::*;
use genawaiter::rc;
use neolith::protocol as proto;
use std::{
    future::Future,
    io::{self, prelude::*, Cursor},
};

type ParseResponse<O> = (usize, Option<O>);
trait Parser {
    type Output;
    fn parse(&mut self, buf: &[u8]) -> ParseResponse<Self::Output>;
}

fn add_to_cursor<C: Buf + Write>(buf: &[u8], cursor: &mut C) -> usize {
    let need = cursor.remaining();
    let len = need.min(buf.len());
    let buf = &buf[..len];
    cursor.write_all(buf).expect("failed to copy to cursor");
    len
}

#[derive(Default)]
pub struct HeaderParser(Cursor<[u8; Self::SIZE]>);
impl HeaderParser {
    const SIZE: usize = size_of::<<Self as Parser>::Output>();
}
impl Parser for HeaderParser {
    type Output = proto::TransactionHeader;
    fn parse(&mut self, buf: &[u8]) -> ParseResponse<Self::Output> {
        let Self(cursor) = self;
        let len = add_to_cursor(buf, cursor);
        if cursor.remaining() == 0 {
            let (_, header) = proto::TransactionHeader::from_bytes((cursor.get_ref(), 0))
                .expect("failed to read header");
            (len, Some(header))
        } else {
            (len, None)
        }
    }
}

#[derive(Default)]
pub struct ParameterCountParser(Cursor<[u8; Self::SIZE]>);
impl ParameterCountParser {
    const SIZE: usize = size_of::<<Self as Parser>::Output>();
}
impl Parser for ParameterCountParser {
    type Output = u16;
    fn parse(&mut self, buf: &[u8]) -> ParseResponse<Self::Output> {
        let Self(cursor) = self;
        let len = add_to_cursor(buf, cursor);
        if cursor.remaining() == 0 {
            let val = u16::from_be_bytes(*cursor.get_ref());
            (len, Some(val))
        } else {
            (len, None)
        }
    }
}

#[derive(Default)]
pub struct FieldIdParser(Cursor<[u8; Self::SIZE]>);
impl FieldIdParser {
    const SIZE: usize = size_of::<<Self as Parser>::Output>();
}
impl Parser for FieldIdParser {
    type Output = proto::FieldId;
    fn parse(&mut self, buf: &[u8]) -> ParseResponse<Self::Output> {
        let Self(cursor) = self;
        let len = add_to_cursor(buf, cursor);
        if cursor.remaining() == 0 {
            let (_, id) =
                proto::FieldId::from_bytes((cursor.get_ref(), 0)).expect("failed to read field id");
            (len, Some(id))
        } else {
            (len, None)
        }
    }
}

#[derive(Default)]
pub struct FieldSizeParser(Cursor<[u8; Self::SIZE]>);
impl FieldSizeParser {
    const SIZE: usize = size_of::<<Self as Parser>::Output>();
}
impl Parser for FieldSizeParser {
    type Output = u16;
    fn parse(&mut self, buf: &[u8]) -> ParseResponse<Self::Output> {
        let Self(cursor) = self;
        let len = add_to_cursor(buf, cursor);
        if cursor.remaining() == 0 {
            let val = u16::from_be_bytes(*cursor.get_ref());
            (len, Some(val))
        } else {
            (len, None)
        }
    }
}

pub struct FieldDataParser(Option<Cursor<Box<[u8]>>>);
impl FieldDataParser {
    pub fn new(data_size: usize) -> Self {
        Self(Some(Cursor::new(vec![0u8; data_size].into_boxed_slice())))
    }
}
impl Parser for FieldDataParser {
    type Output = Box<[u8]>;
    fn parse(&mut self, buf: &[u8]) -> ParseResponse<Self::Output> {
        let mut cursor = self.0.take().expect("cursor is gone");
        let len = add_to_cursor(buf, &mut cursor);
        if cursor.remaining() == 0 {
            let param = cursor.into_inner();
            (len, Some(param))
        } else {
            self.0.replace(cursor);
            (len, None)
        }
    }
}

enum TransactionParseState {
    Header(HeaderParser),
    ParameterCount(ParameterCountParser),
    ParameterFieldId(FieldIdParser),
    ParameterFieldSize(FieldSizeParser),
    ParameterFieldData(FieldDataParser),
}

impl Default for TransactionParseState {
    fn default() -> Self {
        Self::Header(HeaderParser::default())
    }
}

impl std::fmt::Debug for TransactionParseState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Header(_) => "Header",
            Self::ParameterCount(_) => "ParameterCount",
            Self::ParameterFieldId(_) => "ParameterFieldId",
            Self::ParameterFieldSize(_) => "ParameterFieldSize",
            Self::ParameterFieldData(_) => "ParameterFieldData",
        };
        f.write_str(text)
    }
}

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
                field_size: 0,
                field_data: vec![],
            },
        }
    }
}
impl Parser for TransactionParser {
    type Output = proto::TransactionFrame;
    fn parse(&mut self, buf: &[u8]) -> ParseResponse<Self::Output> {
        let len = match &mut self.state {
            TransactionParseState::Header(parser) => {
                let (len, header) = parser.parse(buf);
                let Some(header) = header else {
                    return (len, None);
                };
                self.state = TransactionParseState::ParameterCount(ParameterCountParser::default());
                self.hdr.replace(header);
                len
            }
            TransactionParseState::ParameterCount(parser) => {
                let (len, count) = parser.parse(buf);
                let Some(count) = count else {
                    return (len, None);
                };
                if count == 0 {
                    self.state = TransactionParseState::Header(HeaderParser::default());
                    let header = self.hdr.take().unwrap();
                    let transaction = proto::TransactionFrame::empty(header);
                    return (len, Some(transaction));
                }
                self.param_count = count as usize;
                self.state = TransactionParseState::ParameterFieldId(FieldIdParser::default());
                len
            }
            TransactionParseState::ParameterFieldId(parser) => {
                let (len, id) = parser.parse(buf);
                let Some(id) = id else {
                    return (len, None);
                };
                self.current_param.field_id = id;
                self.state = TransactionParseState::ParameterFieldSize(FieldSizeParser::default());
                len
            }
            TransactionParseState::ParameterFieldSize(parser) => {
                let (len, size) = parser.parse(buf);
                let Some(size) = size else {
                    return (len, None);
                };
                self.current_param.field_size = size;
                self.state =
                    TransactionParseState::ParameterFieldData(FieldDataParser::new(size as usize));
                len
            }
            TransactionParseState::ParameterFieldData(parser) => {
                let (len, field_data) = parser.parse(buf);
                let Some(field_data) = field_data else {
                    return (len, None);
                };
                self.current_param.field_data = field_data.to_vec();
                self.params.push(self.current_param.clone());
                self.param_count = self.param_count.saturating_sub(1);
                if self.param_count == 0 {
                    let header = self.hdr.take().unwrap();
                    let transaction = proto::TransactionFrame::new(header, self.params.clone());
                    self.params = vec![];
                    self.state = TransactionParseState::Header(HeaderParser::default());
                    return (len, Some(transaction));
                }
                self.state = TransactionParseState::ParameterFieldId(FieldIdParser::default());
                len
            }
        };
        (len, None)
    }
}

fn read_client_handshake<R: Read>(r: &mut R) -> io::Result<()> {
    let mut handshake = [0u8; 12];
    r.read_exact(&mut handshake)?;
    Ok(())
}

#[derive(Clone)]
struct BoxedSlice(Box<[u8]>, usize);

impl BoxedSlice {
    fn is_empty(&self) -> bool {
        self.1 == 0
    }
}

impl AsRef<[u8]> for BoxedSlice {
    fn as_ref(&self) -> &[u8] {
        let Self(data, len) = self;
        &data.as_ref()[..*len]
    }
}

async fn parse_co(
    input: rc::Gen<BoxedSlice, (), impl Future<Output = ()>>,
    co: genawaiter::rc::Co<proto::TransactionFrame>,
) {
    let mut parser = TransactionParser::default();
    eprintln!("coro new");
    for buf in input {
        eprintln!("waiting for new chunk");
        if buf.is_empty() {
            eprintln!("no more data");
            break;
        }
        eprintln!("have data");
        let mut buf: &[u8] = buf.as_ref();
        while !buf.is_empty() {
            eprintln!("parse");
            let (len, frame) = parser.parse(buf);
            buf = &buf[len..];
            if let Some(frame) = frame {
                eprintln!("frame");
                co.yield_(frame).await;
            }
        }
    }
    eprintln!("coro done");
}

async fn read_blocking_co(co: rc::Co<BoxedSlice>) {
    let mut stdin = io::stdin().lock();
    read_client_handshake(&mut stdin).expect("handshake");
    eprintln!("handshake ok");
    loop {
        let mut buf = [0u8; 128];
        let len = stdin.read(&mut buf).expect("read");
        if len == 0 {
            break;
        }
        let buf = BoxedSlice(Box::new(buf), len);
        co.yield_(buf).await;
    }
    eprintln!("no more stdin");
}

fn main() -> anyhow::Result<()> {
    let reader = rc::Gen::new(read_blocking_co);
    let frames = rc::Gen::new(move |co| parse_co(reader, co));
    for frame in frames {
        eprintln!("frame {:?}", frame);
    }
    eprintln!("done");
    Ok(())
}
