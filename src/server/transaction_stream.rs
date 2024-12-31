use crate::protocol::{self as proto, HotlineProtocol as _};

use async_stream::stream;
use futures::stream::Stream;
use tokio::io::{AsyncRead, AsyncReadExt as _};

pub type Result<T> = core::result::Result<T, proto::ProtocolError>;

pub struct Frames<R>(R);

impl<R: AsyncRead + Unpin> Frames<R> {
    pub fn new(reader: R) -> Self {
        Self(reader)
    }
    pub fn take(self) -> R {
        self.0
    }
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
        let Self(reader) = self;
        let mut buf = [0u8; 20];
        reader.read_exact(&mut buf).await?;
        proto::TransactionHeader::from_bytes(&buf)
    }
    async fn body(&mut self, size: usize) -> Result<proto::TransactionBody> {
        let Self(reader) = self;
        let buf = &mut vec![0u8; size][..size];
        reader.read_exact(buf).await?;
        proto::TransactionBody::from_bytes(buf)
    }
}
