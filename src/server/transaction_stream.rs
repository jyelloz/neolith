use std::{
    io::{self, Read},
    marker::PhantomData,
};

use crate::protocol::{self as proto, HotlineProtocol as _};

use async_stream::stream;
use deku::DekuSize as _;
use futures::stream::Stream;
use tokio::io::{AsyncRead, AsyncReadExt as _};

pub type Result<T> = core::result::Result<T, proto::ProtocolError>;

pub struct Frames<'r, R>(PhantomData<&'r R>, R);

impl<'r, R> Frames<'r, R> {
    pub fn new(reader: R) -> Self {
        Self(PhantomData, reader)
    }
}

impl<'r, R: Read> Frames<'r, R> {
    fn header_sync(&mut self) -> Result<Option<proto::TransactionHeader>> {
        let Self(_, reader) = self;
        let mut buf = [0u8; proto::TransactionHeader::SIZE_BYTES.unwrap()];
        match reader.read_exact(&mut buf) {
            Ok(_) => {}
            Err(e) => match e.kind() {
                io::ErrorKind::UnexpectedEof => return Ok(None),
                _ => Err(e)?,
            },
        }
        proto::TransactionHeader::from_bytes(&buf).map(Some)
    }
    fn body_sync(&mut self, size: usize) -> Result<proto::TransactionBody> {
        let Self(_, reader) = self;
        let buf = &mut vec![0u8; size][..size];
        reader.read_exact(buf)?;
        proto::TransactionBody::from_bytes(buf)
    }
    fn next_frame_sync(&mut self) -> Result<Option<proto::TransactionFrame>> {
        let Some(header) = self.header_sync()? else {
            return Ok(None);
        };
        let body = self.body_sync(header.body_len())?;
        Ok(Some(proto::TransactionFrame { header, body }))
    }
}

impl<'r, R: Read> Iterator for Frames<'r, R> {
    type Item = Result<proto::TransactionFrame>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_frame_sync() {
            Ok(Some(f)) => Some(Ok(f)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

impl<'r, R: AsyncRead + Unpin + Send> Frames<'r, R> {
    pub fn frames(mut self) -> impl Stream<Item = Result<proto::TransactionFrame>> {
        stream! {
            loop {
                yield self.next_frame().await;
            }
        }
    }
    pub async fn next_frame(&mut self) -> Result<proto::TransactionFrame> {
        let header = self.header().await?;
        let size = header.body_len();
        let body = self.body(size).await?;
        Ok(proto::TransactionFrame { header, body })
    }
    async fn header(&mut self) -> Result<proto::TransactionHeader> {
        let Self(_, reader) = self;
        let mut buf = [0u8; proto::TransactionHeader::SIZE_BYTES.unwrap()];
        reader.read_exact(&mut buf).await?;
        proto::TransactionHeader::from_bytes(&buf)
    }
    async fn body(&mut self, size: usize) -> Result<proto::TransactionBody> {
        let Self(_, reader) = self;
        let buf = &mut vec![0u8; size][..size];
        reader.read_exact(buf).await?;
        proto::TransactionBody::from_bytes(buf)
    }
}
