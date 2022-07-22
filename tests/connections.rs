//! These tests verify whether a node containing a client config can initiate outbound
//! connections, and one containing a server config can accept inbound ones.

mod common;

use std::time::Duration;

use futures_util::StreamExt;
use quickie::*;
use tokio::time::timeout;

#[tokio::test]
#[should_panic]
async fn conns_double_none_config() {
    // a node with neither client nor server config
    let _node = common::TestNode(Node::new(Config::new(None, None)));
}

#[tokio::test]
async fn conns_server_only() {
    // prepare the configs
    let (client_cfg, server_cfg) = common::client_and_server_config();

    // a node in server-only mode
    let node = common::TestNode(Node::new(Config::new(None, Some(server_cfg.clone()))));
    let node_addr = node.start("127.0.0.1:0".parse().unwrap()).await.unwrap();

    // a raw endpoint
    let (raw_endpoint, _raw_incoming) = common::raw_endpoint(client_cfg, server_cfg);
    let raw_endpoint_addr = raw_endpoint.local_addr().unwrap();

    // a server-only node can't initiate a connection
    assert!(node
        .connect(raw_endpoint_addr, common::SERVER_NAME)
        .await
        .is_err());

    // a server-only node can accept a connection
    assert!(raw_endpoint
        .connect(node_addr, common::SERVER_NAME)
        .unwrap()
        .await
        .is_ok());

    // find the inbound connection
    wait_until!(1, node.num_connections() == 1);
    let conn_id = node.get_connections().pop().unwrap().stable_id();

    // check a node-side disconnect
    assert!(node.disconnect(conn_id, Default::default(), &[0]));
    assert!(node.get_connection(conn_id).is_none());
}

#[tokio::test]
async fn conns_client_only() {
    // prepare the configs
    let (client_cfg, server_cfg) = common::client_and_server_config();

    // a node in client-only mode
    let node = common::TestNode(Node::new(Config::new(Some(client_cfg.clone()), None)));
    let node_addr = node.start("127.0.0.1:0".parse().unwrap()).await.unwrap();

    // a raw endpoint
    let (raw_endpoint, mut raw_incoming) = common::raw_endpoint(client_cfg, server_cfg);
    let raw_endpoint_addr = raw_endpoint.local_addr().unwrap();

    // a client-only node can't accept a connection
    // impose a timeout, as it takes 3s otherwise
    assert!(timeout(
        Duration::from_millis(50),
        raw_endpoint
            .connect(node_addr, common::SERVER_NAME)
            .unwrap()
    )
    .await
    .is_err());

    // a client-only node can initiate a connection
    let conn_id = node
        .connect(raw_endpoint_addr, common::SERVER_NAME)
        .await
        .unwrap();

    // make sure that the raw endpoint can finalize the connection too
    assert!(raw_incoming.next().await.unwrap().await.is_ok());

    // check a node-side disconnect
    assert!(node.disconnect(conn_id, Default::default(), &[0]));
    assert!(node.get_connection(conn_id).is_none());
}

#[tokio::test]
async fn conns_client_plus_server() {
    // prepare the configs
    let (client_cfg, server_cfg) = common::client_and_server_config();

    // a node in client+server mode
    let node = common::TestNode(Node::new(Config::new(
        Some(client_cfg.clone()),
        Some(server_cfg.clone()),
    )));
    let node_addr = node.start("127.0.0.1:0".parse().unwrap()).await.unwrap();

    // a raw endpoint
    let (raw_endpoint, mut raw_incoming) = common::raw_endpoint(client_cfg, server_cfg);
    let raw_endpoint_addr = raw_endpoint.local_addr().unwrap();

    // a client+server node can initiate a connection
    let conn_id = node
        .connect(raw_endpoint_addr, common::SERVER_NAME)
        .await
        .unwrap();

    // make sure that the raw endpoint can finalize the connection too
    assert!(raw_incoming.next().await.unwrap().await.is_ok());

    // check a node-side disconnect
    assert!(node.disconnect(conn_id, Default::default(), &[0]));
    assert!(node.get_connection(conn_id).is_none());

    // a client+server node can accept a connection
    assert!(raw_endpoint
        .connect(node_addr, common::SERVER_NAME)
        .unwrap()
        .await
        .is_ok());

    // find the inbound connection
    wait_until!(1, node.num_connections() == 1);
    let conn_id = node.get_connections().pop().unwrap().stable_id();

    // check a node-side disconnect
    assert!(node.disconnect(conn_id, Default::default(), &[0]));
    assert!(node.get_connection(conn_id).is_none());
}
