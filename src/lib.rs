//! A simple, low-level, and customizable implementation of a QUIC P2P node.

#![deny(missing_docs)]
#![deny(unsafe_code)]

mod conn;
mod node;
mod stats;

use std::{
    io,
    net::{SocketAddr, UdpSocket},
};

use bytes::Bytes;
use futures_util::StreamExt;
use quinn::{
    Connecting, Datagrams, Endpoint, IncomingBiStreams, IncomingUniStreams, NewConnection,
    RecvStream, SendStream, StreamId,
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite};
use tracing::*;

use crate::conn::{Conn, Sid, WrappedOutboundMsg};
use crate::stats::{counting_send, CountingDecoder};

pub use crate::{
    conn::ConnId,
    node::{Config, Node},
    stats::StreamStats,
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
        stream_id: StreamId,
        message: Self::InboundMsg,
    ) -> io::Result<()>;

    /// Processes a datagram from a connection.
    async fn process_datagram(&self, source: ConnId, datagram: Bytes) -> io::Result<()>;

    /// Opens a unidirectional stream with the given connection and returns the resulting
    /// send stream's ID.
    async fn open_uni(&self, conn_id: ConnId) -> io::Result<StreamId> {
        if let Some(conn) = self.get_connection(conn_id) {
            match conn.open_uni().await {
                Ok(stream) => {
                    let stream_id = stream.id();
                    self.handle_send_stream(conn_id, stream).await;

                    Ok(stream_id)
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
    /// send and receive streams' IDs.
    async fn open_bi(&self, conn_id: ConnId) -> io::Result<(StreamId, StreamId)> {
        if let Some(conn) = self.get_connection(conn_id) {
            match conn.open_bi().await {
                Ok((send_stream, recv_stream)) => {
                    let send_stream_id = send_stream.id();
                    self.handle_send_stream(conn_id, send_stream).await;
                    let recv_stream_id = recv_stream.id();
                    self.handle_recv_stream(conn_id, recv_stream).await;

                    Ok((send_stream_id, recv_stream_id))
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
        stream_id: StreamId,
        msg: Self::OutboundMsg,
    ) -> io::Result<()> {
        if let Some(streams) = self
            .node()
            .conns
            .read()
            .get(&conn_id)
            .map(|c| c.streams.clone())
        {
            if let Some(stream) = streams.read().get(&stream_id) {
                if let Some(tx) = &stream.msg_sender {
                    if tx.send(Box::new(msg)).is_err() {
                        error!(
                            "send stream {} is known, but its channel is closed",
                            Sid(conn_id, stream_id)
                        );
                        streams.write().remove(&stream_id);

                        Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("stream {} is broken", Sid(conn_id, stream_id)),
                        ))
                    } else {
                        // TODO: provide an rx for stricter delivery tracking
                        Ok(())
                    }
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("{} is not a send stream", Sid(conn_id, stream_id)),
                    ))
                }
            } else {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("stream {} doesn't exist", Sid(conn_id, stream_id)),
                ))
            }
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("connection {:#x} doesn't exist", conn_id),
            ))
        }
    }

    /// Sends the provided message to the specified stream.
    fn send_datagram(&self, conn_id: ConnId, datagram: Bytes) -> io::Result<()> {
        if let Some(conn) = self.get_connection(conn_id) {
            conn.send_datagram(datagram)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("connection {:#x} doesn't exist", conn_id),
            ))
        }
    }

    /// Binds the node to a UDP socket with the given address and returns the bound address.
    async fn start(&self, addr: SocketAddr) -> io::Result<SocketAddr> {
        // create the QUIC endpoint
        let (mut endpoint, incoming) = if let Some(server_cfg) = self.node().config.server.clone() {
            let (endpoint, incoming) = Endpoint::server(server_cfg, addr)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            (endpoint, Some(incoming))
        } else {
            let endpoint =
                Endpoint::client(addr).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            (endpoint, None)
        };

        if let Some(client_cfg) = self.node().config.client.clone() {
            endpoint.set_default_client_config(client_cfg);
        }

        let local_addr = endpoint.local_addr()?;
        self.node().endpoint.set(Box::new(endpoint)).unwrap();

        if let Some(mut inc) = incoming {
            let (tx, rx) = oneshot::channel();
            let node = self.clone();
            let task = tokio::spawn(async move {
                trace!("spawned the listening task");
                tx.send(()).unwrap(); // safe; the channel was just opened

                while let Some(conn) = inc.next().await {
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
        }

        Ok(local_addr)
    }

    /// Switches the node to a new UDP socket; works the same way as in [quinn](https://docs.rs/quinn/latest/quinn/struct.Endpoint.html#method.rebind).
    fn rebind(&self, socket: UdpSocket) -> io::Result<()> {
        self.node().get_endpoint()?.rebind(socket)
    }

    /// Returns the local address the node's socket is bound to.
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.node().get_endpoint()?.local_addr()
    }

    /// Connects the node to the given address and returns its stable ID; the `server_name`
    /// param is the same as the one in [quinn](https://docs.rs/quinn/latest/quinn/struct.Endpoint.html#method.connect).
    async fn connect(&self, addr: SocketAddr, server_name: &str) -> io::Result<ConnId> {
        let conn = self
            .node()
            .get_endpoint()?
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

    /// Returns the number of live [Connection]s.
    fn num_connections(&self) -> usize {
        self.node().conns.read().len()
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

    /// Returns a list of stream IDs corresponding to the given connection ID.
    fn get_stream_ids(&self, conn_id: ConnId) -> Option<Vec<StreamId>> {
        self.node()
            .conns
            .read()
            .get(&conn_id)
            .map(|conn| conn.streams.clone())
            .map(|streams| streams.read().keys().copied().collect())
    }

    /// Returns simple statistics related to the specified stream.
    fn get_stream_stats(&self, conn_id: ConnId, stream_id: StreamId) -> Option<StreamStats> {
        self.node()
            .get_streams(conn_id)
            .and_then(|streams| streams.read().get(&stream_id).map(|s| s.stats.get_stats()))
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
            streams: Default::default(),
            tasks: Default::default(),
        };

        self.node().conns.write().insert(conn_id, conn);

        Ok(conn_id)
    }

    #[doc(hidden)]
    async fn handle_recv_stream(&self, conn_id: ConnId, stream: RecvStream) {
        let stream_id = stream.id();

        let (tx, rx) = oneshot::channel();
        let node = self.clone();
        let task = tokio::spawn(async move {
            trace!(
                "starting a handler task for recv stream {}",
                Sid(conn_id, stream_id)
            );

            let decoder = node.decoder(conn_id, stream_id);
            let framed = FramedRead::new(stream, decoder);
            let mut framed = framed.map_decoder(CountingDecoder::new);

            let _ = rx.await;

            while let Some(item) = framed.next().await {
                match item {
                    Ok((msg, msg_size)) => {
                        node.node().register_msg_rx(conn_id, stream_id, msg_size);

                        trace!(
                            "isolated a {}B message from {}",
                            msg_size,
                            Sid(conn_id, stream_id)
                        );
                        // TODO: send the message to a task dedicated to further processing
                        let node_clone = node.clone();
                        if let Err(e) = node_clone
                            .process_inbound_msg(conn_id, stream_id, msg)
                            .await
                        {
                            error!(
                                "can't process a message from {}: {}",
                                Sid(conn_id, stream_id),
                                e
                            );
                        }
                    }
                    Err(e) => {
                        error!("can't read from {}: {}", Sid(conn_id, stream_id), e);
                    }
                }
            }

            debug!("recv stream {} was closed", Sid(conn_id, stream_id));
        });

        if let Some(conn) = self.node().conns.read().get(&conn_id) {
            conn.register_recv_stream(stream_id, task);
        }

        tx.send(()).unwrap();
    }

    #[doc(hidden)]
    async fn handle_send_stream(&self, conn_id: ConnId, stream: SendStream) {
        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel::<WrappedOutboundMsg>();
        let stream_id = stream.id();

        let (tx, rx) = oneshot::channel();
        let node = self.clone();
        let task = tokio::spawn(async move {
            trace!(
                "starting a handler task for send stream {}",
                Sid(conn_id, stream_id)
            );

            let codec = node.encoder(conn_id, stream_id);
            let mut framed = FramedWrite::new(stream, codec);

            let _ = rx.await;

            while let Some(msg) = msg_rx.recv().await {
                let msg = *msg.downcast().unwrap();

                match counting_send(&mut framed, msg).await {
                    Ok(msg_size) => {
                        node.node().register_msg_tx(conn_id, stream_id, msg_size);

                        trace!(
                            "sent a {}B message to {}",
                            msg_size,
                            Sid(conn_id, stream_id)
                        );
                    }
                    Err(e) => {
                        error!(
                            "couldn't send a message to {}: {}",
                            Sid(conn_id, stream_id),
                            e
                        );
                        break;
                    }
                }
            }

            debug!("send stream {} was closed", Sid(conn_id, stream_id));
        });

        if let Some(conn) = self.node().conns.read().get(&conn_id) {
            conn.register_send_stream(stream_id, task, msg_tx);
        }

        tx.send(()).unwrap();
    }

    /// Closes the given stream.
    fn close_stream(&self, conn_id: ConnId, stream_id: StreamId) -> bool {
        if let Some(streams) = self
            .node()
            .conns
            .read()
            .get(&conn_id)
            .map(|c| c.streams.clone())
        {
            if let Some(stream) = streams.write().remove(&stream_id) {
                if let Some(handle) = stream.recv_task {
                    handle.abort();
                }
                if let Some(handle) = stream.send_task {
                    handle.abort();
                }

                debug!("stream {} was closed", Sid(conn_id, stream_id));
                true
            } else {
                warn!("stream {} doesn't exist", Sid(conn_id, stream_id));
                false
            }
        } else {
            warn!("wasn't connected to {:#x}", conn_id);
            false
        }
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
                    Err(e) => {
                        error!("unidir stream error from {:#x}: {}", conn_id, e);
                        break;
                    }
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
                    Err(e) => {
                        error!("bidir stream error from {:#x}: {}", conn_id, e);
                        break;
                    }
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
                    Err(e) => {
                        error!("incoming datagram error from {:#x}: {}", conn_id, e);
                        break;
                    }
                }
            }
        });

        let _ = rx.await;
        self.node().register_conn_task(conn_id, task);
    }
}
