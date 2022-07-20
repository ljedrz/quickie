use std::sync::Arc;

use quinn::{ClientConfig, ServerConfig, VarInt};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};

pub const SERVER_NAME: &str = "test_server";

const MAX_IDLE_TIMEOUT_MS: u32 = 250;

pub fn start_logger(default_level: LevelFilter) {
    let filter = match EnvFilter::try_from_default_env() {
        Ok(filter) => filter
            .add_directive("tokio_util=off".parse().unwrap())
            .add_directive("quinn=off".parse().unwrap()),
        _ => EnvFilter::default()
            .add_directive(default_level.into())
            .add_directive("tokio_util=off".parse().unwrap())
            .add_directive("quinn=off".parse().unwrap()),
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .without_time()
        .with_target(false)
        .init();
}

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

pub fn client_config(server_cert: Vec<u8>) -> ClientConfig {
    let mut certs = rustls::RootCertStore::empty();
    certs.add(&rustls::Certificate(server_cert)).unwrap();
    let mut config = ClientConfig::with_root_certificates(certs);

    Arc::get_mut(&mut config.transport)
        .unwrap()
        .max_idle_timeout(Some(VarInt::from_u32(MAX_IDLE_TIMEOUT_MS).into()));

    config
}
