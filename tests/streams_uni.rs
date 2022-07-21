//! These tests concentrate on unidirectional streams.

mod common;

use futures_util::StreamExt;
use quickie::*;
use quinn::{Endpoint, NewConnection};

const NUM_MESSAGES: u8 = 3;

#[tokio::test]
async fn streams_uni_outbound() {
    // prepare the configs
    let (client_cfg, server_cfg) = common::client_and_server_config();

    // a node in client-only mode
    let node = common::TestNode(Node::new(Config::new(Some(client_cfg), None)));
    node.start("127.0.0.1:0".parse().unwrap()).await.unwrap();

    // a raw endpoint in server-only mode
    let (raw_server, mut raw_server_incoming) =
        Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).unwrap();
    let raw_server_addr = raw_server.local_addr().unwrap();

    // initiate a connection
    let conn_id = node
        .connect(raw_server_addr, common::SERVER_NAME)
        .await
        .unwrap();

    // open a uni stream
    let stream_id = node.open_uni(conn_id).await.unwrap();

    // send a few messages
    for i in 0..NUM_MESSAGES {
        node.unicast(conn_id, stream_id, [i].to_vec().into())
            .unwrap();
    }

    // get the corresponding uni stream on the raw endpoint side
    let NewConnection {
        mut uni_streams, ..
    } = raw_server_incoming.next().await.unwrap().await.unwrap();
    let mut recv_stream = uni_streams.next().await.unwrap().unwrap();

    // check if the raw endpoint got all of the messages
    let mut recv_buf = [255u8];
    for i in 0..NUM_MESSAGES {
        recv_stream.read_exact(&mut recv_buf).await.unwrap();
        assert_eq!(recv_buf[0], i);
    }
}
