//! A simple, low-level, and customizable implementation of a QUIC P2P node.

mod conn;
mod node;

use std::{io, net::SocketAddr};

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use quinn::{
    Connecting, Datagrams, Endpoint, IncomingBiStreams, IncomingUniStreams, NewConnection,
    RecvStream, SendStream, ServerConfig, StreamId,
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite};
use tracing::*;

use crate::conn::Conn;
pub use crate::{
    conn::{ConnId, StreamIdx},
    node::Node,
};
pub use quinn::{Connection, VarInt};

/// A trait for objects containing a [Node]; it endows them with P2P networking
/// capabilities.
#[async_trait::async_trait]
pub trait Quickie
where
    Self: Clone + Send + 'static,
{
    /// The type of the messages read from the network; it can be raw bytes or
    /// any final (deserialized) type; it is bound to the [Decoder].
    type InboundMsg: Send;

    /// The user-supplied [Decoder] used to interpret inbound messages.
    type Decoder: Decoder<Item = Self::InboundMsg, Error = io::Error> + Send;

    /// The type of the messages sent over the network; it can be any initial
    /// (non-serialized) type or raw bytes; it is bound to the [Encoder].
    type OutboundMsg: Send;

    /// The user-supplied [Encoder] used to write outbound messages.
    type Encoder: Encoder<Self::OutboundMsg, Error = io::Error> + Send;

    /// Returns a clonable reference to the [Node] which provides the owning
    /// object with P2P networking capabilities.
    fn node(&self) -> &Node;

    /// Creates a [Decoder] used to read messages from the given stream.
    fn decoder(&self, conn_id: ConnId, stream_id: StreamId) -> Self::Decoder;

    /// Creates an [Encoder] used to write messages to the given stream.
    fn encoder(&self, conn_id: ConnId, stream_id: StreamId) -> Self::Encoder;

    /// Processes an inbound message from a network stream.
    async fn process_inbound_msg(
        &self,
        conn_id: ConnId,
        stream_idx: StreamIdx,
        message: Self::InboundMsg,
    ) -> io::Result<()>;

    /// Processes a datagram from a connection.
    async fn process_datagram(&self, source: ConnId, datagram: Bytes) -> io::Result<()>;

    /// Opens a unidirectional stream with the given connection and returns the resulting
    /// send stream's index.
    async fn open_uni(&self, conn_id: ConnId) -> io::Result<StreamIdx> {
        if let Some(conn) = self.get_connection(conn_id) {
            match conn.open_uni().await {
                Ok(stream) => {
                    let stream_idx = stream.id().index();
                    self.handle_send_stream(conn_id, stream).await;

                    Ok(stream_idx)
                }
                Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
            }
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("connection {:#x} doesn't exist", conn_id),
            ))
        }
    }

    /// Opens a bidirectional stream with the given connection and returns the resulting
    /// send and receive streams' indices.
    async fn open_bi(&self, conn_id: ConnId) -> io::Result<(StreamIdx, StreamIdx)> {
        if let Some(conn) = self.get_connection(conn_id) {
            match conn.open_bi().await {
                Ok((send_stream, recv_stream)) => {
                    let send_stream_idx = send_stream.id().index();
                    self.handle_send_stream(conn_id, send_stream).await;
                    let recv_stream_idx = recv_stream.id().index();
                    self.handle_recv_stream(conn_id, recv_stream).await;

                    Ok((send_stream_idx, recv_stream_idx))
                }
                Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
            }
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("connection {:#x} doesn't exist", conn_id),
            ))
        }
    }

    /// Sends the provided message to the specified stream.
    fn unicast(
        &self,
        conn_id: ConnId,
        stream_idx: StreamIdx,
        msg: Self::OutboundMsg,
    ) -> io::Result<()> {
        if let Some(senders) = self
            .node()
            .conns
            .read()
            .get(&conn_id)
            .map(|c| c.senders.clone())
        {
            if let Some(tx) = senders.read().get(&stream_idx) {
                if tx.send(Box::new(msg)).is_err() {
                    error!(
                        "send stream {:#x}:{} is known, but its channel is closed",
                        conn_id, stream_idx
                    );
                    senders.write().remove(&stream_idx);

                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("stream {:#x}:{} is broken", conn_id, stream_idx),
                    ))
                } else {
                    // TODO: provide an rx for stricter delivery tracking
                    Ok(())
                }
            } else {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("stream {:#x}:{} doesn't exist", conn_id, stream_idx),
                ))
            }
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("connection {:#x} doesn't exist", conn_id),
            ))
        }
    }

    /// Binds the node to a UDP socket with the given address and server config.
    async fn listen(&self, addr: SocketAddr, config: ServerConfig) -> io::Result<SocketAddr> {
        // create the QUIC endpoint
        let (endpoint, mut incoming) = Endpoint::server(config, addr).unwrap();
        let local_addr = endpoint.local_addr()?;
        self.node().endpoint.set(Box::new(endpoint)).unwrap();

        let (tx, rx) = oneshot::channel();
        let node = self.clone();
        let task = tokio::spawn(async move {
            trace!("spawned the listening task");
            tx.send(()).unwrap(); // safe; the channel was just opened

            while let Some(conn) = incoming.next().await {
                let addr = conn.remote_address();
                trace!("received a connection attempt from {}", addr);

                let node_clone = node.clone();
                tokio::spawn(async move {
                    if let Err(e) = node_clone.process_conn(conn).await {
                        error!("rejected a connection attempt from {}: {}", addr, e);
                    }
                });
            }
        });

        self.node().register_task(task);
        let _ = rx.await;

        Ok(local_addr)
    }

    /// Connects the node to the given address and returns its stable ID; the `server_name`
    /// param is the same as the one in [quinn](https://docs.rs/quinn/latest/quinn/struct.Endpoint.html#method.connect).
    async fn connect(&self, addr: SocketAddr, server_name: &str) -> io::Result<ConnId> {
        let conn = self
            .node()
            .endpoint
            .get()
            .unwrap()
            .connect(addr, server_name)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        self.process_conn(conn).await
    }

    /// Disconnects the node from a connection with the given stable ID; the `error_code` and
    /// the `reason` are the same as in [quinn](https://docs.rs/quinn/latest/quinn/struct.Connection.html#method.close).
    async fn disconnect(&self, conn_id: ConnId, error_code: VarInt, reason: &[u8]) -> bool {
        if let Some(conn) = self.node().conns.write().remove(&conn_id) {
            debug!("disconnecting from {:#x}", conn_id);

            conn.conn.close(error_code, reason);

            for task in conn.tasks.lock().drain(..) {
                task.abort();
            }

            true
        } else {
            warn!("wasn't connected to {:#x}", conn_id);

            false
        }
    }

    /// Returns a list of live [Connection]s.
    fn get_connections(&self) -> Vec<Connection> {
        self.node()
            .conns
            .read()
            .values()
            .map(|c| c.conn.clone())
            .collect()
    }

    /// Returns a [Connection] corresponding to the given stable connection ID.
    fn get_connection(&self, conn_id: ConnId) -> Option<Connection> {
        self.node()
            .conns
            .read()
            .get(&conn_id)
            .map(|c| c.conn.clone())
    }

    #[doc(hidden)]
    async fn process_conn(&self, conn: Connecting) -> io::Result<ConnId> {
        let addr = conn.remote_address();
        trace!("finalizing connection with {}", addr);

        let new_conn = conn
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let conn_id = new_conn.connection.stable_id();

        debug!(
            "successfully connected to {}; stable ID: {:#x}",
            addr, conn_id
        );

        let NewConnection {
            connection,
            uni_streams,
            bi_streams,
            datagrams,
            ..
        } = new_conn;

        let (n1, n2, n3) = (self.clone(), self.clone(), self.clone());
        tokio::join!(
            n1.handle_uni_streams(conn_id, uni_streams),
            n2.handle_bi_streams(conn_id, bi_streams),
            n3.handle_datagrams(conn_id, datagrams),
        );

        let conn = Conn {
            conn: connection,
            senders: Default::default(),
            tasks: Default::default(),
        };

        self.node().conns.write().insert(conn_id, conn);

        Ok(conn_id)
    }

    #[doc(hidden)]
    async fn handle_recv_stream(&self, conn_id: ConnId, stream: RecvStream) {
        let (tx, rx) = oneshot::channel();
        let node = self.clone();
        let task = tokio::spawn(async move {
            let stream_id = stream.id();
            let stream_idx = stream_id.index();

            let codec = node.decoder(conn_id, stream_id);
            let mut framed = FramedRead::new(stream, codec);

            tx.send(()).unwrap();

            while let Some(bytes) = framed.next().await {
                match bytes {
                    Ok(msg) => {
                        // TODO: send the message to a task dedicated to further processing
                        let node_clone = node.clone();
                        if let Err(e) = node_clone
                            .process_inbound_msg(conn_id, stream_idx, msg)
                            .await
                        {
                            error!(
                                "can't process a message from {:#x}:{}: {}",
                                conn_id, stream_idx, e
                            );
                        }
                    }
                    Err(e) => {
                        error!("can't read from {:#x}:{}: {}", conn_id, stream_idx, e);
                    }
                }
            }

            debug!("receive stream {:#x}:{} was closed", conn_id, stream_idx,);
        });

        let _ = rx.await;
        self.node().register_conn_task(conn_id, task);
    }

    #[doc(hidden)]
    async fn handle_send_stream(&self, conn_id: ConnId, stream: SendStream) {
        let (tx, rx) = oneshot::channel();
        let node = self.clone();
        let task = tokio::spawn(async move {
            let stream_id = stream.id();
            let stream_idx = stream_id.index();

            let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();
            if let Some(conn) = node.node().conns.write().get(&conn_id) {
                conn.senders.write().insert(stream_idx, msg_tx);
            } else {
                return;
            }

            let codec = node.encoder(conn_id, stream_id);
            let mut framed = FramedWrite::new(stream, codec);

            tx.send(()).unwrap();

            while let Some(msg) = msg_rx.recv().await {
                let msg = *msg.downcast().unwrap();

                if let Err(e) = framed.send(msg).await {
                    error!(
                        "couldn't send a message to {:#x}:{}: {}",
                        conn_id, stream_idx, e
                    );
                }
            }

            if let Some(senders) = node
                .node()
                .conns
                .read()
                .get(&conn_id)
                .map(|c| c.senders.clone())
            {
                senders.write().remove(&stream_idx);
            }

            debug!("send stream {:#x}:{} was closed", conn_id, stream_idx,);
        });

        let _ = rx.await;
        self.node().register_conn_task(conn_id, task);
    }

    #[doc(hidden)]
    async fn handle_uni_streams(&self, conn_id: ConnId, mut streams: IncomingUniStreams) {
        let (tx, rx) = oneshot::channel();
        let node = self.clone();
        let task = tokio::spawn(async move {
            trace!("handling unidir streams from {:#x}", conn_id);
            tx.send(()).unwrap(); // safe; the channel was just opened

            while let Some(item) = streams.next().await {
                match item {
                    Ok(recv_stream) => {
                        trace!("received a unidir stream from {:#x}", conn_id);
                        node.handle_recv_stream(conn_id, recv_stream).await;
                    }
                    Err(e) => error!("unidir stream error from {:#x}: {}", conn_id, e),
                }
            }
        });

        let _ = rx.await;
        self.node().register_conn_task(conn_id, task);
    }

    #[doc(hidden)]
    async fn handle_bi_streams(&self, conn_id: ConnId, mut streams: IncomingBiStreams) {
        let (tx, rx) = oneshot::channel();
        let node = self.clone();
        let task = tokio::spawn(async move {
            trace!("handling bidir streams from {:#x}", conn_id);
            tx.send(()).unwrap(); // safe; the channel was just opened

            while let Some(item) = streams.next().await {
                match item {
                    Ok((send_stream, recv_stream)) => {
                        debug!("received a bidir stream from {:#x}", conn_id);
                        node.handle_send_stream(conn_id, send_stream).await;
                        node.handle_recv_stream(conn_id, recv_stream).await;
                    }
                    Err(e) => error!("bidir stream error from {:#x}: {}", conn_id, e),
                }
            }
        });

        let _ = rx.await;
        self.node().register_conn_task(conn_id, task);
    }

    #[doc(hidden)]
    async fn handle_datagrams(&self, conn_id: ConnId, mut datagrams: Datagrams) {
        let (tx, rx) = oneshot::channel();
        let node = self.clone();
        let task = tokio::spawn(async move {
            trace!("handling datagrams from {:#x}", conn_id);
            tx.send(()).unwrap(); // safe; the channel was just opened

            while let Some(item) = datagrams.next().await {
                match item {
                    Ok(datagram) => {
                        if let Err(e) = node.process_datagram(conn_id, datagram).await {
                            error!("failed to process a datagram from {:#x}: {}", conn_id, e);
                        }
                    }
                    Err(e) => error!("incoming datagram error from {:#x}: {}", conn_id, e),
                }
            }
        });

        let _ = rx.await;
        self.node().register_conn_task(conn_id, task);
    }
}
