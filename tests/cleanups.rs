mod common;

use std::time::Duration;

use bytes::Bytes;
use deadline::deadline;
use humansize::{format_size, ToF64, Unsigned, DECIMAL};
use peak_alloc::PeakAlloc;
use quickie::*;

#[global_allocator]
static PEAK_ALLOC: PeakAlloc = PeakAlloc;

fn fmt_size(size: impl ToF64 + Unsigned) -> String {
    format_size(size, DECIMAL)
}

#[tokio::test]
async fn cleanups_conns() {
    const NUM_CONNS: usize = 100;

    // register heap use before node setup
    let initial_heap = PEAK_ALLOC.current_usage();

    // prepare the configs
    let (client_cfg, server_cfg) = common::client_and_server_config();

    // a node in client-only mode
    let node = common::TestNode(Node::new(Config::new(
        Some(client_cfg.clone()),
        Some(server_cfg.clone()),
    )));

    // measure the size of a node with no connections
    let idle_node_size = PEAK_ALLOC.current_usage() - initial_heap;

    let node_addr = node.start("127.0.0.1:0".parse().unwrap()).await.unwrap();

    // register heap use after node setup
    let heap_after_node_setup = PEAK_ALLOC.current_usage();

    // start keeping track of the average heap use
    let mut avg_heap = 0;

    // due to tokio channel internals, a small heap bump occurs after 32 calls to `mpsc::Sender::send`
    // if it wasn't for that, heap use after the 1st connection (i == 0) would be registered instead
    let mut heap_after_32_conns = 0;

    for i in 0..NUM_CONNS {
        // a raw endpoint
        let raw_endpoint = common::raw_endpoint(client_cfg.clone(), server_cfg.clone());
        let raw_endpoint_addr = raw_endpoint.local_addr().unwrap();

        // test both outbound and inbound conns
        let (conn_id, connection, raw_endpoint) = if i % 2 == 0 {
            // prepare to accept a connection at the raw endpoint
            let connection_and_endpoint = tokio::spawn(async move {
                if let Some(conn) = raw_endpoint.accept().await {
                    (conn.await.unwrap(), raw_endpoint)
                } else {
                    panic!("failed to accept a connection");
                }
            });

            let conn_id = node
                .connect(raw_endpoint_addr, common::SERVER_NAME)
                .await
                .unwrap();

            // make sure that the raw endpoint can finalize the connection too
            let (connection, endpoint) = connection_and_endpoint.await.unwrap();

            (conn_id, connection, endpoint)
        } else {
            let connection = raw_endpoint
                .connect(node_addr, common::SERVER_NAME)
                .unwrap()
                .await
                .unwrap();

            let node_clone = node.clone();
            deadline!(Duration::from_secs(1), move || node_clone.num_connections()
                == 1);
            let conn_id = node.get_connections().pop().unwrap().stable_id();

            (conn_id, connection, raw_endpoint)
        };

        // check both outbond and inbound uni streams
        if i % 2 == 0 {
            let stream_id = node.open_uni(conn_id).await.unwrap();
            node.send_msg(conn_id, stream_id, Bytes::from_static(b"herp derp"))
                .unwrap();
        } else {
            let mut send_stream = connection.open_uni().await.unwrap();
            send_stream.write_all(b"herp derp").await.unwrap();
        }

        assert!(node.disconnect(conn_id, Default::default(), &[]).await);

        connection.close(Default::default(), &[]);
        connection.closed().await;
        raw_endpoint.close(Default::default(), &[]);
        raw_endpoint.wait_idle().await;

        // obtain and record current memory use
        let current_heap = PEAK_ALLOC.current_usage();

        // register heap use once the 33rd connection is established and dropped
        if i == 32 {
            heap_after_32_conns = current_heap;
        }

        // save current heap size to calculate average use later on
        avg_heap += current_heap;
    }

    // calculate avg heap use
    avg_heap /= NUM_CONNS;

    // check final heap use and calculate heap growth
    let final_heap = PEAK_ALLOC.current_usage();
    let heap_growth = final_heap.saturating_sub(heap_after_32_conns);

    // calculate some helper values
    let max_heap = PEAK_ALLOC.peak_usage();
    let single_node_size = heap_after_node_setup - initial_heap;

    println!("---- heap use summary ----\n");
    println!("before node setup:     {}", fmt_size(initial_heap));
    println!("after node setup:      {}", fmt_size(heap_after_node_setup));
    println!("after 32 connections:  {}", fmt_size(heap_after_32_conns));
    println!("after {} connections: {}", NUM_CONNS, fmt_size(final_heap));
    println!("average memory use:    {}", fmt_size(avg_heap));
    println!("maximum memory use:    {}", fmt_size(max_heap)); // note: heavily affected by Config::initial_read_buffer_size
    println!();
    println!("idle node size:    {}", fmt_size(idle_node_size));
    println!("started node size: {}", fmt_size(single_node_size));
    println!("leaked memory:     {}", fmt_size(heap_growth));
    println!();

    // regardless of the number of connections the node handles, its memory use shouldn't grow at all
    assert_eq!(heap_growth, 0);
}
