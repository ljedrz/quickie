use std::{collections::HashMap, io, ops::Deref, sync::Arc};

use once_cell::race::OnceBox;
use parking_lot::{Mutex, RwLock};
use quinn::{ClientConfig, Endpoint, ServerConfig, StreamId};
use tokio::task::JoinHandle;

use crate::conn::{Conn, ConnId, Streams};

/// Contains objects providing P2P networking capabilities.
#[derive(Clone)]
pub struct Node(Arc<InnerNode>);

impl Deref for Node {
    type Target = Arc<InnerNode>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[doc(hidden)]
pub struct InnerNode {
    pub(crate) config: Config,
    pub(crate) endpoint: OnceBox<Endpoint>,
    pub(crate) conns: RwLock<HashMap<ConnId, Conn>>,
    pub(crate) tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Node {
    /// Creates an idle node with the given config.
    pub fn new(config: Config) -> Self {
        Self(Arc::new(InnerNode {
            config,
            endpoint: Default::default(),
            conns: Default::default(),
            tasks: Default::default(),
        }))
    }

    /// Returns a handle to the QUIC endpoint.
    pub(crate) fn get_endpoint(&self) -> io::Result<&Endpoint> {
        self.endpoint.get().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Other,
                "no existing socket found; you must call `Node::start` first",
            )
        })
    }

    /// Registers the given node-level task.
    pub(crate) fn register_task(&self, handle: JoinHandle<()>) {
        self.tasks.lock().push(handle);
    }

    /// Registers the given connection-level task.
    pub(crate) fn register_conn_task(&self, conn_id: ConnId, handle: JoinHandle<()>) {
        if let Some(tasks) = self.conns.read().get(&conn_id).map(|c| c.tasks.clone()) {
            tasks.lock().push(handle);
        }
    }

    /// Returns the list of streams applicable to the given connection.
    pub(crate) fn get_streams(&self, conn_id: ConnId) -> Option<Arc<Streams>> {
        self.conns.read().get(&conn_id).map(|c| c.streams.clone())
    }

    /// Registers an inbound message from the given stream.
    pub(crate) fn register_msg_rx(&self, conn_id: ConnId, stream_id: StreamId, size: usize) {
        if let Some(stats) = self
            .get_streams(conn_id)
            .and_then(|streams| streams.read().get(&stream_id).map(|s| s.stats.clone()))
        {
            stats.register_msg_rx(size);
        }
    }

    /// Registers an outbound message to the given stream.
    pub(crate) fn register_msg_tx(&self, conn_id: ConnId, stream_id: StreamId, size: usize) {
        if let Some(stats) = self
            .get_streams(conn_id)
            .and_then(|streams| streams.read().get(&stream_id).map(|s| s.stats.clone()))
        {
            stats.register_msg_tx(size);
        }
    }
}

/// The configuration for the node.
#[derive(Debug, Clone)]
pub struct Config {
    /// Default client configuration (mandatory to allow outbound connections).
    pub(crate) client: Option<ClientConfig>,
    /// Server configuration (mandatory to allow inbound connections).
    pub(crate) server: Option<ServerConfig>,
}

impl Config {
    /// Creates a new node config object. The arguments can't be `None` at the same time
    /// (otherwise the node couldn't do anything).
    pub fn new(client: Option<ClientConfig>, server: Option<ServerConfig>) -> Self {
        if client.is_none() && server.is_none() {
            panic!("the node can't function without any config");
        }

        Self { client, server }
    }
}
