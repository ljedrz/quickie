use std::{any::Any, collections::HashMap, sync::Arc};

use parking_lot::{Mutex, RwLock};
use quinn::Connection;
use tokio::{sync::mpsc, task::JoinHandle};

/// The stable ID of a connection.
pub type ConnId = usize;

/// The index identifying a network stream within a connection.
pub type StreamIdx = u64;

type OutboundMsgSender = mpsc::UnboundedSender<Box<dyn Any + Send>>;

pub(crate) struct Conn {
    pub(crate) conn: Connection,
    pub(crate) senders: Arc<RwLock<HashMap<StreamIdx, OutboundMsgSender>>>,
    pub(crate) tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
}
