use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

use super::protocol::{
    HotlineProtocol as _, ProtocolError, TransactionBody, TransactionFrame, TransactionHeader,
};

type Result<T> = ::core::result::Result<T, ProtocolError>;

pub struct Connection<S> {
    socket: S,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Connection<S> {
    pub async fn read_frame(&mut self) -> Result<TransactionFrame> {
        let header = self.header().await?;
        let size = header.body_len();
        let body = self.body(size).await?;
        Ok(TransactionFrame { header, body })
    }
    async fn header(&mut self) -> Result<TransactionHeader> {
        let Self { socket } = self;
        let mut buf = [0u8; 20];
        socket.read_exact(&mut buf).await?;
        match TransactionHeader::try_from(&buf[..]) {
            Ok(header) => Ok(header),
            Err(_) => Err(ProtocolError::ParseHeader),
        }
    }
    async fn body(&mut self, size: usize) -> Result<TransactionBody> {
        let Self { socket } = self;
        let mut buf = vec![0u8; size];
        socket.read_exact(&mut buf[..size]).await?;
        match TransactionBody::try_from(&buf[..]) {
            Ok(body) => Ok(body),
            Err(_) => Err(ProtocolError::ParseBody),
        }
    }
    pub async fn write_frame(&mut self, frame: TransactionFrame) -> Result<()> {
        self.socket.write_all(&frame.into_bytes()).await?;
        Ok(())
    }
}
