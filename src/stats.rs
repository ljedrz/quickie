use std::io;

use bytes::BytesMut;
use futures_util::SinkExt;
use tokio::io::AsyncWrite;
use tokio_util::codec::{Decoder, Encoder, FramedWrite};

/// A wrapper [`Decoder`] that also counts the bytes belonging to the inbound messages.
pub(crate) struct CountingDecoder<D: Decoder> {
    decoder: D,
    acc: usize,
}

impl<D: Decoder> CountingDecoder<D> {
    pub(crate) fn new(decoder: D) -> Self {
        Self { decoder, acc: 0 }
    }
}

impl<D: Decoder> Decoder for CountingDecoder<D> {
    type Item = (D::Item, usize);
    type Error = D::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let initial_buf_len = src.len();
        let ret = self.decoder.decode(src)?;
        let final_buf_len = src.len();
        let read_len = initial_buf_len - final_buf_len + self.acc;

        if read_len != 0 {
            if let Some(item) = ret {
                self.acc = 0;

                Ok(Some((item, read_len)))
            } else {
                self.acc = read_len;

                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}

pub(crate) async fn counting_send<
    T: Send,
    W: AsyncWrite + Unpin,
    E: Encoder<T, Error = io::Error>,
>(
    framed: &mut FramedWrite<W, E>,
    item: T,
) -> io::Result<usize> {
    framed.feed(item).await?;
    let len = framed.write_buffer().len();
    framed.flush().await?;
    Ok(len)
}
