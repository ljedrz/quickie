//! An minimal setup capable of connecting to a libp2p-quic node.

mod common;

use std::sync::Arc;

use futures_util::StreamExt;
use libp2p::core::multiaddr;
use libp2p::{
    swarm::{dummy::Behaviour, SwarmEvent},
    SwarmBuilder,
};
use quickie::*;
use quinn::ClientConfig;
use quinn_proto::crypto::rustls::QuicClientConfig;
use tokio::sync::oneshot;
use tracing::*;
use tracing_subscriber::filter::LevelFilter;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // start the logger
    common::start_logger(LevelFilter::TRACE);

    // prepare the libp2p config
    let keypair = libp2p::identity::Keypair::generate_ed25519();

    let mut swarm = SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_quic()
        .with_behaviour(|_key| Behaviour)
        .unwrap()
        .build();

    swarm
        .listen_on("/ip4/127.0.0.1/udp/0/quic-v1".parse().unwrap())
        .unwrap();

    // listen for events in the libp2p node
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        let mut tx = Some(tx);
        loop {
            let event = swarm.select_next_some().await;
            debug!("libp2p node: {:?}", event);
            if let SwarmEvent::NewListenAddr { address, .. } = event {
                tx.take().unwrap().send(address).unwrap();
            }
        }
    });

    // obtain the listening port of the libp2p node
    let mut addr = rx.await.unwrap();
    addr.pop(); // drop the final Quic bit
    let port = if let Some(multiaddr::Protocol::Udp(port)) = addr.pop() {
        port
    } else {
        panic!("the libp2p swarm did not return a listening UDP port");
    };

    // prepare a quickie client config adhering to the libp2p setup
    let crypto = libp2p_tls::make_client_config(&keypair, None).unwrap();
    let client_cfg = ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto).unwrap()));

    // create a quickie node in client-only mode and start it
    let node = common::TestNode(Node::new(Config::new(Some(client_cfg), None)));
    node.start("127.0.0.1:0".parse().unwrap()).await.unwrap();

    // initiate a connection
    let _conn_id = node
        .connect(format!("127.0.0.1:{port}").parse().unwrap(), "l") // this is the domain currently used by libp2p-quic
        .await
        .unwrap();
}
