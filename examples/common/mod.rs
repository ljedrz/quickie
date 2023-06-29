#![allow(unused)]

use std::{io, sync::Arc, time::Duration};

use bytes::{Bytes, BytesMut};
use quickie::{ConnId, Node, Quickie};
use quinn::{ClientConfig, Endpoint, ServerConfig, StreamId, TransportConfig};
use tokio_util::codec::BytesCodec;
use tracing::*;
use tracing_subscriber::filter::{EnvFilter, LevelFilter};

/// A basic test node.
#[derive(Clone)]
pub struct TestNode(pub Node);

#[async_trait::async_trait]
impl Quickie for TestNode {
    type InboundMsg = BytesMut;
    type Decoder = BytesCodec;

    type OutboundMsg = Bytes;
    type Encoder = BytesCodec;

    fn node(&self) -> &Node {
        &self.0
    }

    fn decoder(&self, _conn_id: ConnId, _stream_id: StreamId) -> Self::Decoder {
        Default::default()
    }

    fn encoder(&self, _conn_id: ConnId, _stream_id: StreamId) -> Self::Encoder {
        Default::default()
    }

    async fn process_inbound_msg(
        &self,
        conn_id: ConnId,
        stream_id: StreamId,
        message: Self::InboundMsg,
    ) -> io::Result<()> {
        info!(
            "got a message from {:#x} on {}: {:?}",
            conn_id, stream_id, message
        );

        Ok(())
    }

    async fn process_datagram(&self, source: ConnId, datagram: Bytes) -> io::Result<()> {
        info!("got a datagram from {:#x}: {:?}", source, datagram);

        Ok(())
    }
}

pub fn start_logger(level: LevelFilter) {
    let filter = match EnvFilter::try_from_default_env() {
        Ok(filter) => filter
            .add_directive("tokio_util=off".parse().unwrap())
            .add_directive("quinn=off".parse().unwrap()),
        _ => EnvFilter::default()
            .add_directive(level.into())
            .add_directive("tokio_util=off".parse().unwrap())
            .add_directive("quinn=off".parse().unwrap()),
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .without_time()
        .with_target(false)
        .init();
}
