#![allow(unused)]

use std::{io, sync::Arc, time::Duration};

use bytes::{Bytes, BytesMut};
use quickie::{ConnId, Node, Quickie};
use quinn::{ClientConfig, Endpoint, ServerConfig, StreamId, TransportConfig};
use quinn_proto::crypto::rustls::QuicClientConfig;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use tokio_util::codec::BytesCodec;
use tracing::*;
use tracing_subscriber::filter::{EnvFilter, LevelFilter};

pub const SERVER_NAME: &str = "test_server";

const MAX_IDLE_TIMEOUT_MS: u64 = 250;

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

mod danger {
    use std::sync::Arc;

    use rustls::client::danger::HandshakeSignatureValid;
    use rustls::crypto::{verify_tls12_signature, verify_tls13_signature};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::DigitallySignedStruct;

    #[derive(Debug)]
    pub(super) struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

    impl SkipServerVerification {
        pub(super) fn new(provider: Arc<rustls::crypto::CryptoProvider>) -> Arc<Self> {
            Arc::new(Self(provider))
        }
    }

    impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp: &[u8],
            _now: UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            verify_tls12_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            verify_tls13_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }
}

/// Creates a server config and its corresponding certificate in DER form.
pub fn server_config_and_cert() -> (ServerConfig, CertificateDer<'static>) {
    let cert = rcgen::generate_simple_self_signed(vec![SERVER_NAME.into()]).unwrap();
    let cert_der = CertificateDer::from(cert.cert);
    let priv_key = PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());

    let mut server_config =
        ServerConfig::with_single_cert(vec![cert_der.clone()], priv_key.into()).unwrap();
    let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
    transport_config.max_idle_timeout(Some(
        Duration::from_millis(MAX_IDLE_TIMEOUT_MS)
            .try_into()
            .unwrap(),
    ));

    (server_config, cert_der)
}

/// Creates a client config compatible with the given certificate.
pub fn client_config(server_cert: Vec<u8>) -> ClientConfig {
    let mut certs = rustls::RootCertStore::empty();
    certs.add(CertificateDer::from(server_cert)).unwrap();

    let mut config = ClientConfig::with_root_certificates(Arc::new(certs)).unwrap();
    let mut transport_cfg = TransportConfig::default();
    transport_cfg.max_idle_timeout(Some(
        Duration::from_millis(MAX_IDLE_TIMEOUT_MS)
            .try_into()
            .unwrap(),
    ));
    config.transport_config(Arc::new(transport_cfg));

    config
}

/// Creates a client config ignoring the server certificate
pub fn insecure_client_config() -> ClientConfig {
    let provider = rustls::crypto::ring::default_provider();
    provider.install_default();
    let provider = rustls::crypto::CryptoProvider::get_default().unwrap();
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(danger::SkipServerVerification::new(provider.clone()))
        .with_no_client_auth();

    ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto).unwrap()))
}

/// Creates a compatible pair of client and server configs.
pub fn client_and_server_config() -> (ClientConfig, ServerConfig) {
    let (server_cfg, _server_cert) = server_config_and_cert();
    let client_cfg = insecure_client_config();
    // let client_cfg = client_config((*server_cert).to_owned());

    (client_cfg, server_cfg)
}

/// Creates a raw `quinn` endpoint capable of initiating and accepting connections.
pub fn raw_endpoint(client_cfg: ClientConfig, server_cfg: ServerConfig) -> Endpoint {
    let mut endpoint = Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(client_cfg);

    endpoint
}
