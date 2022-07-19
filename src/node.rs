use std::{collections::HashMap, ops::Deref, sync::Arc};

use once_cell::race::OnceBox;
use parking_lot::{Mutex, RwLock};
use quinn::Endpoint;
use tokio::task::JoinHandle;

use crate::conn::{Conn, ConnId};

/// Contains objects providing P2P networking capabilities.
#[derive(Clone, Default)]
pub struct Node(Arc<InnerNode>);

impl Deref for Node {
    type Target = Arc<InnerNode>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[doc(hidden)]
#[derive(Default)]
pub struct InnerNode {
    pub(crate) endpoint: OnceBox<Endpoint>,
    pub(crate) conns: RwLock<HashMap<ConnId, Conn>>,
    pub(crate) tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Node {
    pub(crate) fn register_task(&self, handle: JoinHandle<()>) {
        self.tasks.lock().push(handle);
    }

    pub(crate) fn register_conn_task(&self, conn_id: ConnId, handle: JoinHandle<()>) {
        if let Some(tasks) = self.conns.read().get(&conn_id).map(|c| c.tasks.clone()) {
            tasks.lock().push(handle);
        }
    }
}
