//! These tests concentrate on unidirectional streams.

mod common;

use std::time::Duration;

use futures_util::StreamExt;
use quickie::*;
use quinn::NewConnection;
use tokio::time::sleep;

const NUM_MESSAGES: u8 = 3;

#[tokio::test]
async fn streams_uni() {
    // prepare the configs
    let (client_cfg, server_cfg) = common::client_and_server_config();

    // a node in client-only mode
    let node = common::TestNode(Node::new(Config::new(Some(client_cfg.clone()), None)));
    node.start("127.0.0.1:0".parse().unwrap()).await.unwrap();

    // a raw endpoint
    let (raw_endpoint, mut raw_incoming) = common::raw_endpoint(client_cfg, server_cfg);
    let raw_endpoint_addr = raw_endpoint.local_addr().unwrap();

    // initiate a connection
    let conn_id = node
        .connect(raw_endpoint_addr, common::SERVER_NAME)
        .await
        .unwrap();

    // accept it on the raw endpoint side
    let NewConnection {
        connection,
        mut uni_streams,
        ..
    } = raw_incoming.next().await.unwrap().await.unwrap();

    // send messages to a uni stream
    {
        // open a uni stream
        let stream_id = node.open_uni(conn_id).await.unwrap();

        // send a few messages
        for i in 0..NUM_MESSAGES {
            node.send_msg(conn_id, stream_id, [i, i].to_vec().into())
                .unwrap();
        }

        wait_until!(1, {
            let stats = node.get_stream_stats(conn_id, stream_id).unwrap();
            stats.msgs_sent == NUM_MESSAGES as u64 && stats.bytes_sent == NUM_MESSAGES as u64 * 2
        });

        // get the corresponding uni stream on the raw endpoint side
        let mut raw_recv_stream = uni_streams.next().await.unwrap().unwrap();

        // check if the raw endpoint got all of the messages
        let mut recv_buf = [255u8, 255];
        for i in 0..NUM_MESSAGES {
            raw_recv_stream.read_exact(&mut recv_buf).await.unwrap();
            assert_eq!(recv_buf, [i, i]);
        }

        node.close_stream(conn_id, stream_id);
    }

    // receive messages from a uni stream
    {
        // start a uni stream
        let mut raw_send_stream = connection.open_uni().await.unwrap();

        // send a few messages
        for i in 0..NUM_MESSAGES {
            raw_send_stream.write_all(&[i, i]).await.unwrap();
            // a small delay so that the messages aren't stuck together
            sleep(Duration::from_millis(1)).await;
        }

        let mut stream_ids = node.get_stream_ids(conn_id).unwrap();
        assert_eq!(stream_ids.len(), 1);
        let stream_id = stream_ids.pop().unwrap();

        wait_until!(1, {
            let stats = node.get_stream_stats(conn_id, stream_id).unwrap();
            stats.msgs_recv == NUM_MESSAGES as u64 && stats.bytes_recv == NUM_MESSAGES as u64 * 2
        });
    }
}

#[tokio::test]
async fn streams_bi() {
    // prepare the configs
    let (client_cfg, server_cfg) = common::client_and_server_config();

    // a node in client-only mode
    let node = common::TestNode(Node::new(Config::new(Some(client_cfg.clone()), None)));
    node.start("127.0.0.1:0".parse().unwrap()).await.unwrap();

    // a raw endpoint
    let (raw_endpoint, mut raw_incoming) = common::raw_endpoint(client_cfg, server_cfg);
    let raw_endpoint_addr = raw_endpoint.local_addr().unwrap();

    // initiate a connection
    let conn_id = node
        .connect(raw_endpoint_addr, common::SERVER_NAME)
        .await
        .unwrap();

    // accept it on the raw endpoint side
    let NewConnection {
        connection,
        mut bi_streams,
        ..
    } = raw_incoming.next().await.unwrap().await.unwrap();

    // send and receive messages in an outbound bi stream
    {
        // open a bi stream
        let stream_id = node.open_bi(conn_id).await.unwrap();

        // send a few messages
        for i in 0..NUM_MESSAGES {
            node.send_msg(conn_id, stream_id, [i, i].to_vec().into())
                .unwrap();
        }

        wait_until!(1, {
            let stats = node.get_stream_stats(conn_id, stream_id).unwrap();
            stats.msgs_sent == NUM_MESSAGES as u64 && stats.bytes_sent == NUM_MESSAGES as u64 * 2
        });

        // get the corresponding bi stream on the raw endpoint side
        let (mut raw_send_stream, mut raw_recv_stream) = bi_streams.next().await.unwrap().unwrap();

        // check if the raw endpoint got all of the messages
        let mut recv_buf = [255u8, 255];
        for i in 0..NUM_MESSAGES {
            raw_recv_stream.read_exact(&mut recv_buf).await.unwrap();
            assert_eq!(recv_buf, [i, i]);
        }

        // send a few messages
        for i in 0..NUM_MESSAGES {
            raw_send_stream.write_all(&[i, i]).await.unwrap();
            // a small delay so that the messages aren't stuck together
            sleep(Duration::from_millis(1)).await;
        }

        wait_until!(1, {
            let stats = node.get_stream_stats(conn_id, stream_id).unwrap();
            stats.msgs_recv == NUM_MESSAGES as u64 && stats.bytes_recv == NUM_MESSAGES as u64 * 2
        });

        node.close_stream(conn_id, stream_id);
    }

    // receive and send messages in an inbound bi stream
    {
        // start a bi stream
        let (mut raw_send_stream, mut raw_recv_stream) = connection.open_bi().await.unwrap();

        // send a few messages
        for i in 0..NUM_MESSAGES {
            raw_send_stream.write_all(&[i, i]).await.unwrap();
            // a small delay so that the messages aren't stuck together
            sleep(Duration::from_millis(1)).await;
        }

        let mut stream_ids = node.get_stream_ids(conn_id).unwrap();
        assert_eq!(stream_ids.len(), 1);
        let stream_id = stream_ids.pop().unwrap();

        wait_until!(1, {
            let stats = node.get_stream_stats(conn_id, stream_id).unwrap();
            stats.msgs_recv == NUM_MESSAGES as u64 && stats.bytes_recv == NUM_MESSAGES as u64 * 2
        });

        // send a few messages
        for i in 0..NUM_MESSAGES {
            node.send_msg(conn_id, stream_id, [i, i].to_vec().into())
                .unwrap();
        }

        wait_until!(1, {
            let stats = node.get_stream_stats(conn_id, stream_id).unwrap();
            stats.msgs_recv == NUM_MESSAGES as u64 && stats.bytes_recv == NUM_MESSAGES as u64 * 2
        });

        // check if the raw endpoint got all of the messages
        let mut recv_buf = [255u8, 255];
        for i in 0..NUM_MESSAGES {
            raw_recv_stream.read_exact(&mut recv_buf).await.unwrap();
            assert_eq!(recv_buf, [i, i]);
        }
    }
}
