mod common;

use std::{io, time::Duration};

use bytes::{Bytes, BytesMut};
use futures_util::{sink::SinkExt, StreamExt};
use quickie::*;
use quinn::{ClientConfig, Endpoint, ServerConfig, StreamId};
use tokio::time::sleep;
use tokio_util::codec::{BytesCodec, FramedWrite};
use tracing::*;
use tracing_subscriber::filter::LevelFilter;

#[derive(Default, Clone)]
struct TestNode(Node);

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
        stream_idx: StreamIdx,
        message: Self::InboundMsg,
    ) -> io::Result<()> {
        info!(
            "got a message from {:#x}:{} {:?}",
            conn_id, stream_idx, message
        );

        Ok(())
    }

    async fn process_datagram(&self, source: ConnId, datagram: Bytes) -> io::Result<()> {
        info!("got a datagram from {:#x}: {:?}", source, datagram);

        Ok(())
    }
}

// a temporary test that only checks that the library "basically works"
#[tokio::test]
async fn temp() {
    common::start_logger(LevelFilter::TRACE);

    // configure the QUIC server
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert_der = cert.serialize_der().unwrap();
    let priv_key = cert.serialize_private_key_der();
    let priv_key = rustls::PrivateKey(priv_key);
    let cert_chain = vec![rustls::Certificate(cert_der.clone())];
    let server_config = ServerConfig::with_single_cert(cert_chain, priv_key).unwrap();
    /* TODO: check out
    Arc::get_mut(&mut server_config.transport)
        .unwrap()
        .max_concurrent_uni_streams(0_u8.into());
    */

    let node = TestNode::default();
    let node_addr = node
        .listen("127.0.0.1:0".parse().unwrap(), server_config)
        .await
        .unwrap();

    let client = {
        let mut certs = rustls::RootCertStore::empty();
        certs.add(&rustls::Certificate(cert_der.to_vec())).unwrap();
        let config = ClientConfig::with_root_certificates(certs);

        let mut client = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        client.set_default_client_config(config);

        client
    };

    let mut client_conn = client
        .connect(node_addr, "localhost")
        .unwrap()
        .await
        .unwrap();
    let send_stream = client_conn.connection.open_uni().await.unwrap();
    let mut framed = FramedWrite::new(send_stream, BytesCodec::default());

    let msg = b"herp derp";
    framed.send(Bytes::from(&msg[..])).await.unwrap();

    sleep(Duration::from_millis(100)).await;

    let conn_id = node.get_connections().pop().unwrap().stable_id();
    let stream_idx = node.open_uni(conn_id).await.unwrap();
    node.unicast(conn_id, stream_idx, Bytes::from(&msg[..]))
        .unwrap();

    let mut msg_recv = [0u8; 9];
    client_conn
        .uni_streams
        .next()
        .await
        .unwrap()
        .unwrap()
        .read_exact(&mut msg_recv)
        .await
        .unwrap();

    assert_eq!(&msg_recv, msg);

    sleep(Duration::from_millis(100)).await;
}
