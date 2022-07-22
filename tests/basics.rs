mod common;

use std::time::Duration;

use bytes::Bytes;
use futures_util::{sink::SinkExt, StreamExt};
use quickie::*;
use quinn::{Endpoint, NewConnection};
use tokio::time::sleep;
use tokio_util::codec::{BytesCodec, FramedWrite};

// a temporary test that only checks that the server side "basically works"
#[tokio::test]
async fn temp_server_side_comms() {
    // a node in server mode
    let (server_cfg, server_cert) = common::server_config_and_cert();
    let node = common::TestNode(Node::new(Config::new(None, Some(server_cfg))));
    let node_addr = node.start("127.0.0.1:0".parse().unwrap()).await.unwrap();

    // a client endpoint
    let client_cfg = common::client_config(server_cert);
    let mut client = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    client.set_default_client_config(client_cfg);
    let client_addr = client.local_addr().unwrap();

    assert!(node
        .connect(client_addr, common::SERVER_NAME)
        .await
        .is_err());

    let mut client_conn = client
        .connect(node_addr, common::SERVER_NAME)
        .unwrap()
        .await
        .unwrap();
    let send_stream = client_conn.connection.open_uni().await.unwrap();
    let mut framed = FramedWrite::new(send_stream, BytesCodec::default());

    let msg = b"herp derp";
    framed.send(Bytes::from_static(msg)).await.unwrap();

    sleep(Duration::from_millis(100)).await;

    let conn_id = node.get_connections().pop().unwrap().stable_id();
    let stream_id = node.open_uni(conn_id).await.unwrap();
    node.unicast(conn_id, stream_id, Bytes::from_static(msg))
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

// a temporary test that only checks that the server side "basically works"
#[tokio::test]
async fn temp_client_side_comms() {
    // prepare the configs
    let (server_cfg, server_cert) = common::server_config_and_cert();
    let client_cfg = common::client_config(server_cert);

    // an endpoint in server mode
    let (mut server, mut server_incoming) =
        Endpoint::server(server_cfg, "127.0.0.1:0".parse().unwrap()).unwrap();
    server.set_default_client_config(client_cfg.clone());
    let server_addr = server.local_addr().unwrap();

    // a client node
    let node = common::TestNode(Node::new(Config::new(Some(client_cfg), None)));
    let node_addr = node.start("127.0.0.1:0".parse().unwrap()).await.unwrap();

    assert!(server
        .connect(node_addr, common::SERVER_NAME)
        .unwrap()
        .await
        .is_err());

    let conn_id = node
        .connect(server_addr, common::SERVER_NAME)
        .await
        .unwrap();

    let NewConnection {
        mut uni_streams,
        mut bi_streams,
        mut datagrams,
        ..
    } = server_incoming.next().await.unwrap().await.unwrap();

    // check a unidirectional stream
    {
        let send_stream_id = node.open_uni(conn_id).await.unwrap();

        let msg = b"herp derp";
        node.unicast(conn_id, send_stream_id, Bytes::from_static(msg))
            .unwrap();

        let mut recv_stream = uni_streams.next().await.unwrap().unwrap();

        let mut msg_recv = [0u8; 9];
        recv_stream.read_exact(&mut msg_recv).await.unwrap();
        assert_eq!(&msg_recv, msg);
    }

    // check a bidirectional stream
    {
        let stream_id = node.open_bi(conn_id).await.unwrap();

        let msg = b"herp derp";
        node.unicast(conn_id, stream_id, Bytes::from_static(msg))
            .unwrap();

        let (mut send_stream, mut recv_stream) = bi_streams.next().await.unwrap().unwrap();

        let mut msg_recv = [0u8; 9];
        recv_stream.read_exact(&mut msg_recv).await.unwrap();
        assert_eq!(&msg_recv, msg);

        send_stream.write_all(msg).await.unwrap();

        sleep(Duration::from_millis(100)).await;
    }

    // check a datagram
    {
        let msg = b"hurr durr";
        node.send_datagram(conn_id, Bytes::from_static(msg))
            .unwrap();

        assert_eq!(datagrams.next().await.unwrap().unwrap(), &msg[..]);
    }

    sleep(Duration::from_millis(100)).await;
}
