use std::{any::Any, collections::HashMap, fmt, sync::Arc};

use parking_lot::{Mutex, RwLock};
use quinn::{Connection, StreamId};
use quinn_proto::{Dir, Side};
use tokio::{sync::mpsc, task::JoinHandle};

/// The stable ID of a connection.
pub type ConnId = usize;

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

type OutboundMsgSender = mpsc::UnboundedSender<Box<dyn Any + Send>>;

pub(crate) struct Conn {
    pub(crate) conn: Connection,
    pub(crate) senders: Arc<RwLock<HashMap<StreamId, OutboundMsgSender>>>,
    pub(crate) tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
}
