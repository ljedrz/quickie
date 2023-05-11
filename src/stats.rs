use std::{
    io,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
};

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

/// A helper function counting the bytes written to a `FramedWrite`.
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

/// The internal representation of a stream's stats.
#[derive(Default)]
pub(crate) struct StreamStatsInner {
    /// The number of all messages sent.
    msgs_sent: AtomicU64,
    /// The number of all messages received.
    msgs_recv: AtomicU64,
    /// The number of all bytes sent.
    bytes_sent: AtomicU64,
    /// The number of all bytes received.
    bytes_recv: AtomicU64,
}

impl StreamStatsInner {
    pub(crate) fn register_msg_rx(&self, size: usize) {
        self.msgs_recv.fetch_add(1, Relaxed);
        self.bytes_recv.fetch_add(size as u64, Relaxed);
    }

    pub(crate) fn register_msg_tx(&self, size: usize) {
        self.msgs_sent.fetch_add(1, Relaxed);
        self.bytes_sent.fetch_add(size as u64, Relaxed);
    }

    pub(crate) fn get_stats(&self) -> StreamStats {
        StreamStats {
            msgs_sent: self.msgs_sent.load(Relaxed),
            msgs_recv: self.msgs_recv.load(Relaxed),
            bytes_sent: self.bytes_sent.load(Relaxed),
            bytes_recv: self.bytes_recv.load(Relaxed),
        }
    }
}

// TODO: rename to something else, now that datagram stats are collected too.
/// A set of simple statistics related to a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamStats {
    /// The number of all messages sent.
    pub msgs_sent: u64,
    /// The number of all messages received.
    pub msgs_recv: u64,
    /// The number of all bytes sent.
    pub bytes_sent: u64,
    /// The number of all bytes received.
    pub bytes_recv: u64,
}
