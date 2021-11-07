use crate::protocol::{
    HotlineProtocol as _,
    TransactionHeader,
    TransactionBody,
    TransactionFrame,
    ProtocolError,
};

use tokio::io::{
    AsyncRead,
    AsyncReadExt as _,
};
use futures::stream::Stream;
use async_stream::try_stream;

pub type Result<T> = core::result::Result<T, ProtocolError>;

pub struct Frames<R>(R);

impl <R: AsyncRead + Unpin> Frames<R> {
    pub fn new(reader: R) -> Self {
        Self(reader)
    }
    pub fn take(self) -> R {
        self.0
    }
    pub fn frames(mut self) -> impl Stream<Item = Result<TransactionFrame>>{
        try_stream! {
            while let Ok(frame) = self.next_frame().await {
                yield frame;
            }
        }
    }
    pub async fn next_frame(&mut self) -> Result<TransactionFrame> {
        let header = self.header().await?;
        let size = header.body_len();
        let body = self.body(size).await?;
        Ok(TransactionFrame { header, body })
    }
    async fn header(&mut self) -> Result<TransactionHeader> {
        let Self(reader) = self;
        let mut buf = [0u8; 20];
        reader.read_exact(&mut buf).await?;
        match TransactionHeader::from_bytes(&buf) {
            Ok((_, header)) => Ok(header),
            Err(_) => Err(ProtocolError::ParseHeader),
        }
    }
    async fn body(&mut self, size: usize) -> Result<TransactionBody> {
        let Self(reader) = self;
        let mut buf = &mut vec![0u8; size][..size];
        reader.read_exact(&mut buf).await?;
        match TransactionBody::from_bytes(&buf) {
            Ok((_, body)) => Ok(body),
            Err(_) => Err(ProtocolError::ParseBody),
        }
    }
}
