#![allow(unused)]

use std::{io, sync::Arc};

use bytes::{Bytes, BytesMut};
use quickie::{ConnId, Node, Quickie};
use quinn::{ClientConfig, Endpoint, Incoming, ServerConfig, StreamId, VarInt};
use tokio_util::codec::BytesCodec;
use tracing::*;
use tracing_subscriber::filter::{EnvFilter, LevelFilter};

pub const SERVER_NAME: &str = "test_server";

const MAX_IDLE_TIMEOUT_MS: u32 = 250;

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

pub fn start_logger() {
    let filter = EnvFilter::default()
        .add_directive(LevelFilter::TRACE.into())
        .add_directive("tokio_util=off".parse().unwrap())
        .add_directive("quinn=off".parse().unwrap());

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .without_time()
        .with_target(false)
        .init();
}

struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

/// Creates a server config and its corresponding certificate in DER form.
pub fn server_config_and_cert() -> (ServerConfig, Vec<u8>) {
    let cert = rcgen::generate_simple_self_signed(vec![SERVER_NAME.into()]).unwrap();
    let cert_der = cert.serialize_der().unwrap();
    let priv_key = cert.serialize_private_key_der();
    let priv_key = rustls::PrivateKey(priv_key);
    let cert_chain = vec![rustls::Certificate(cert_der.clone())];
    let mut config = ServerConfig::with_single_cert(cert_chain, priv_key).unwrap();

    Arc::get_mut(&mut config.transport)
        .unwrap()
        .max_idle_timeout(Some(VarInt::from_u32(MAX_IDLE_TIMEOUT_MS).into()));

    (config, cert_der)
}

/// Creates a client config compatible with the given certificate.
pub fn client_config(server_cert: Vec<u8>) -> ClientConfig {
    let mut certs = rustls::RootCertStore::empty();
    certs.add(&rustls::Certificate(server_cert)).unwrap();
    let mut config = ClientConfig::with_root_certificates(certs);

    Arc::get_mut(&mut config.transport)
        .unwrap()
        .max_idle_timeout(Some(VarInt::from_u32(MAX_IDLE_TIMEOUT_MS).into()));

    config
}

/// Creates a client config ignoring the server certificate
pub fn insecure_client_config() -> ClientConfig {
    let crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    ClientConfig::new(Arc::new(crypto))
}

/// Creates a compatible pair of client and server configs.
pub fn client_and_server_config() -> (ClientConfig, ServerConfig) {
    let (server_cfg, _server_cert) = server_config_and_cert();
    let client_cfg = insecure_client_config();

    (client_cfg, server_cfg)
}

/// Creates a raw `quinn` endpoint capable of initiating and accepting connections.
pub fn raw_endpoint(client_cfg: ClientConfig, server_cfg: ServerConfig) -> (Endpoint, Incoming) {
    let (mut endpoint, incoming) =
        Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(client_cfg);

    (endpoint, incoming)
}
