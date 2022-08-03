mod common;

use std::time::Duration;

use bytes::Bytes;
use deadline::deadline;
use futures_util::StreamExt;
use peak_alloc::PeakAlloc;
use quickie::*;
use tokio::time::sleep;

#[global_allocator]
static PEAK_ALLOC: PeakAlloc = PeakAlloc;

#[tokio::test]
async fn cleanups_conns() {
    const NUM_CONNS: usize = 100;

    // register heap use before node setup
    let initial_heap_use = PEAK_ALLOC.current_usage();

    // prepare the configs
    let (client_cfg, server_cfg) = common::client_and_server_config();

    // a node in client-only mode
    let node = common::TestNode(Node::new(Config::new(
        Some(client_cfg.clone()),
        Some(server_cfg.clone()),
    )));

    // measure the size of a node with no connections
    let idle_node_size = (PEAK_ALLOC.current_usage() - initial_heap_use) / 1000;

    let node_addr = node.start("127.0.0.1:0".parse().unwrap()).await.unwrap();

    // register heap use after node setup
    let heap_after_node_setup = PEAK_ALLOC.current_usage();

    // start keeping track of the average heap use
    let mut avg_heap_use = 0;

    // due to tokio channel internals, a small heap bump occurs after 32 calls to `mpsc::Sender::send`
    // if it wasn't for that, heap use after the 1st connection (i == 0) would be registered instead
    let mut heap_after_32_conns = 0;

    for i in 0..NUM_CONNS {
        // a raw endpoint
        let (raw_endpoint, mut raw_incoming) =
            common::raw_endpoint(client_cfg.clone(), server_cfg.clone());
        let raw_endpoint_addr = raw_endpoint.local_addr().unwrap();

        // test both outbound and inbound conns
        let (conn_id, raw_new_conn) = if i % 2 == 0 {
            let conn_id = node
                .connect(raw_endpoint_addr, common::SERVER_NAME)
                .await
                .unwrap();

            // make sure that the raw endpoint can finalize the connection too
            let raw_new_conn = raw_incoming.next().await.unwrap().await.unwrap();

            (conn_id, raw_new_conn)
        } else {
            let raw_new_conn = raw_endpoint
                .connect(node_addr, common::SERVER_NAME)
                .unwrap()
                .await
                .unwrap();

            let node_clone = node.clone();
            deadline!(Duration::from_secs(1), move || node_clone.num_connections()
                == 1);
            let conn_id = node.get_connections().pop().unwrap().stable_id();

            (conn_id, raw_new_conn)
        };

        // check both outbond and inbound uni streams
        if i % 2 == 0 {
            let stream_id = node.open_uni(conn_id).await.unwrap();
            node.send_msg(conn_id, stream_id, Bytes::from_static(b"herp derp"))
                .unwrap();
        } else {
            let mut send_stream = raw_new_conn.connection.open_uni().await.unwrap();
            send_stream.write_all(b"herp derp").await.unwrap();
        }

        assert!(node.disconnect(conn_id, Default::default(), &[]).await);

        // TODO: try to avoid this sleep
        sleep(Duration::from_millis(10)).await;

        raw_new_conn.connection.close(Default::default(), &[]);
        raw_endpoint.close(Default::default(), &[]);

        // TODO: try to avoid this sleep
        sleep(Duration::from_millis(20)).await;

        // obtain and record current memory use
        let current_heap_use = PEAK_ALLOC.current_usage();

        // register heap use once the 33rd connection is established and dropped
        if i == 32 {
            heap_after_32_conns = current_heap_use;
        }

        // save current heap size to calculate average use later on
        avg_heap_use += current_heap_use;
    }

    // calculate avg heap use
    avg_heap_use /= NUM_CONNS;

    // check final heap use and calculate heap growth
    let final_heap_use = PEAK_ALLOC.current_usage();
    let heap_growth = final_heap_use - heap_after_32_conns;

    // calculate some helper values
    let max_heap_use = PEAK_ALLOC.peak_usage() / 1000;
    let final_heap_use_kb = final_heap_use / 1000;
    let single_node_size = (heap_after_node_setup - initial_heap_use) / 1000;

    println!("---- heap use summary ----\n");
    println!("before node setup:     {}kB", initial_heap_use / 1000);
    println!("after node setup:      {}kB", heap_after_node_setup / 1000);
    println!("after 32 connections:  {}kB", heap_after_32_conns / 1000);
    println!("after {} connections: {}kB", NUM_CONNS, final_heap_use_kb);
    println!("average memory use:    {}kB", avg_heap_use / 1000);
    println!("maximum memory use:    {}kB", max_heap_use); // note: heavily affected by Config::initial_read_buffer_size
    println!();
    println!("idle node size:    {}kB", idle_node_size);
    println!("started node size: {}kB", single_node_size);
    println!("leaked memory:     {}B", heap_growth);
    println!();

    // regardless of the number of connections the node handles, its memory use shouldn't grow at all
    assert_eq!(heap_growth, 0);
}
