use std::io::{Read, Write};

use deku::DekuSize as _;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

use super::protocol::{
    self as proto, HotlineProtocol as _, ProtocolError, TransactionBody, TransactionFrame,
    TransactionHeader,
};

type Result<T> = ::core::result::Result<T, ProtocolError>;

pub struct Connection<S> {
    stream: S,
}

impl<S> Connection<S> {
    pub fn new(stream: S) -> Self {
        Self { stream }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Connection<S> {
    pub async fn read_frame(&mut self) -> Result<TransactionFrame> {
        let header = self.header().await?;
        let size = header.body_len();
        let body = self.body(size).await?;
        Ok(TransactionFrame { header, body })
    }
    async fn header(&mut self) -> Result<TransactionHeader> {
        let Self { stream } = self;
        let mut buf = [0u8; TransactionHeader::SIZE_BYTES.unwrap()];
        stream.read_exact(&mut buf).await?;
        match TransactionHeader::try_from(&buf[..]) {
            Ok(header) => Ok(header),
            Err(_) => Err(ProtocolError::ParseHeader),
        }
    }
    async fn body(&mut self, size: usize) -> Result<TransactionBody> {
        let Self { stream } = self;
        let mut buf = vec![0u8; size];
        stream.read_exact(&mut buf[..size]).await?;
        match TransactionBody::try_from(&buf[..]) {
            Ok(body) => Ok(body),
            Err(_) => Err(ProtocolError::ParseBody),
        }
    }
    pub async fn write_frame<F: Into<TransactionFrame>>(&mut self, into: F) -> Result<()> {
        let frame = into.into();
        self.stream.write_all(&frame.into_bytes()).await?;
        Ok(())
    }
}

impl<S: Read + Write> Connection<S> {
    pub fn read_frame_sync(&mut self) -> Result<TransactionFrame> {
        let header = self.header_sync()?;
        let size = header.body_len();
        let body = self.body_sync(size)?;
        Ok(TransactionFrame { header, body })
    }
    fn header_sync(&mut self) -> Result<TransactionHeader> {
        let Self { stream } = self;
        let mut buf = [0u8; TransactionHeader::SIZE_BYTES.unwrap()];
        stream.read_exact(&mut buf)?;
        match TransactionHeader::try_from(&buf[..]) {
            Ok(header) => Ok(header),
            Err(_) => Err(ProtocolError::ParseHeader),
        }
    }
    fn body_sync(&mut self, size: usize) -> Result<TransactionBody> {
        let Self { stream } = self;
        let mut buf = vec![0u8; size];
        stream.read_exact(&mut buf[..size])?;
        match TransactionBody::try_from(&buf[..]) {
            Ok(body) => Ok(body),
            Err(_) => Err(ProtocolError::ParseBody),
        }
    }
    pub fn write_frame_sync<F: Into<TransactionFrame>>(&mut self, into: F) -> Result<proto::Id> {
        let frame = into.into();
        let id = frame.header.id;
        self.stream.write_all(&frame.into_bytes())?;
        Ok(id)
    }
}
