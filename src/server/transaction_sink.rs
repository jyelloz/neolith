use crate::protocol::{
    HotlineProtocol as _,
    TransactionFrame,
};

use futures::{
    sink::{Sink, SinkExt},
    io::{
        AsyncWrite as FuturesAsyncWrite,
        AsyncWriteExt as FuturesAsyncWriteExt,
    },
};

use std::io;

pub struct Frames<W>(pub W);

impl <W: FuturesAsyncWrite + Unpin> Frames<W> {
    pub fn hotline_sink(self) -> impl Sink<TransactionFrame, Error=io::Error> {
        let Self(w) = self;
        w.into_sink().with(
            |frame: TransactionFrame| async {
                Ok(frame.into_bytes())
            }
        )
    }
}
