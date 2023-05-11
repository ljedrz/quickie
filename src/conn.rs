use std::{any::Any, collections::HashMap, fmt, sync::Arc};

use parking_lot::{Mutex, RwLock};
use quinn::{Connection, StreamId};
use quinn_proto::{Dir, Side};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::stats::StreamStatsInner;

/// The stable ID of a connection.
pub type ConnId = usize;

/// The set of streams belonging to a connection.
pub(crate) type Streams = RwLock<HashMap<StreamId, Stream>>;

/// A wrapper providing a Display impl for the stable connection ID and stream ID.
pub(crate) struct Sid(pub ConnId, pub StreamId);

impl fmt::Display for Sid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let side = match self.1.initiator() {
            Side::Client => 'c',
            Side::Server => 's',
        };
        let dir = match self.1.dir() {
            Dir::Bi => 'b',
            Dir::Uni => 'u',
        };
        let idx = self.1.index();

        write!(f, "stream {:#x}:{}{}{}", self.0, side, dir, idx)
    }
}

pub(crate) type WrappedOutboundMsg = Box<dyn Any + Send>;
type OutboundMsgSender = mpsc::UnboundedSender<WrappedOutboundMsg>;

/// An object representing a QUIC connection.
pub(crate) struct Conn {
    pub(crate) conn: Connection,
    pub(crate) streams: Arc<Streams>,
    pub(crate) tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
    pub(crate) datagram_stats: Arc<StreamStatsInner>,
}

impl Conn {
    /// Registers a recv stream with the connection.
    pub(crate) fn register_recv_stream(&self, stream_id: StreamId, recv_task: JoinHandle<()>) {
        self.streams.write().entry(stream_id).or_default().recv_task = Some(recv_task)
    }

    /// Registers a send stream with the connection.
    pub(crate) fn register_send_stream(
        &self,
        stream_id: StreamId,
        send_task: JoinHandle<()>,
        msg_sender: OutboundMsgSender,
    ) {
        let mut streams = self.streams.write();
        let stream = streams.entry(stream_id).or_default();
        stream.send_task = Some(send_task);
        stream.msg_sender = Some(msg_sender);
    }
}

/// Represents a QUIC stream, both bi/uni.
#[derive(Default)]
pub(crate) struct Stream {
    pub(crate) recv_task: Option<JoinHandle<()>>,
    pub(crate) send_task: Option<JoinHandle<()>>,
    pub(crate) msg_sender: Option<OutboundMsgSender>,
    pub(crate) stats: Arc<StreamStatsInner>,
}
