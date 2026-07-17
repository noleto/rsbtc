use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use btclib::network::{Connection, DEFAULT_REQUEST_TIMEOUT, Message};
use btclib::types::Blockchain;
use btclib::utils::Saveable;
use tokio::sync::Mutex;
use tokio::time;

use crate::NODES;

pub async fn load_blockchain(blockchain_file: &str) -> Result<()> {
    println!("blockchain file exists, loading...");
    let new_blockchain = Blockchain::load_from_file(blockchain_file)?;

    let mut blockchain = crate::BLOCKCHAIN.write().await;
    *blockchain = new_blockchain;
    println!("rebuilding utxos...");
    blockchain.rebuild_utxos();
    println!("utxos rebuilt");
    println!("checking if target needs to be adjusted...");
    println!("current target: {}", blockchain.target());
    blockchain.try_adjust_target();
    println!("new target: {}", blockchain.target());
    println!("initialization complete");
    Ok(())
}

pub async fn populate_connections(nodes: &[String]) -> Result<()> {
    println!("trying to connect to other nodes...");

    for node_addr in nodes
        .iter()
        //skip duplicate nodes in the list or nodes already connected
        .filter(|n| !NODES.contains_key(*n))
        //limit the number of seeds in case of seed flooding
        .take(btclib::MAX_PEERS_TO_CONNECT)
    {
        println!("connectiong to {node_addr}");

        let mut conn = match Connection::connect(node_addr).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("failed to connect to seed {node_addr}: {e}, skipping");
                continue;
            }
        };

        let child_nodes = match conn
            .request_expect(
                &Message::DiscoverNodes,
                DEFAULT_REQUEST_TIMEOUT,
                |m| match m {
                    Message::NodeList(child_nodes) => Some(child_nodes),
                    _ => None,
                },
            )
            .await
        {
            Ok(child_nodes) => child_nodes,
            Err(e) => {
                eprintln!("failed to DiscoverNodes to {node_addr}: {e}, skipping");
                continue;
            }
        };

        // seed responded correctly — keep its connection
        println!("received NodeList from {node_addr}");
        NODES.insert(node_addr.to_string(), Arc::new(Mutex::new(conn)));

        for child_node in child_nodes
            .iter()
            // TODO should prevent to connect to self
            .filter(|cn| !NODES.contains_key(*cn))
            //prevent malicious nodes from flooding this peer
            .take(btclib::MAX_PEERS_TO_CONNECT)
        {
            match Connection::connect(child_node).await {
                Ok(child_conn) => {
                    println!("adding node {child_node}");
                    NODES.insert(child_node.to_string(), Arc::new(Mutex::new(child_conn)));
                }
                Err(e) => {
                    eprintln!("failed to connect to discovered node {child_node}: {e}")
                }
            };
        }
    }
    Ok(())
}

pub async fn find_longest_chain_node() -> Option<(String, u64)> {
    // copy entries so mutex guards can be released after collect
    let peers = crate::NODES
        .iter()
        .map(|e| (e.key().clone(), e.value().clone()))
        .collect::<Vec<_>>();

    let mut result: Option<(String, u64)> = None;

    for (peer_addr, conn_wrapper) in peers {
        println!("asking {peer_addr} for blockchain height");

        let mut conn = conn_wrapper.lock().await;

        let peer_height = match conn
            .request_expect(
                &&Message::AskBlockCount,
                DEFAULT_REQUEST_TIMEOUT,
                |m| match m {
                    Message::BlockCount(h) => Some(h),
                    _ => None,
                },
            )
            .await
        {
            Ok(h) => h,
            Err(e) => {
                eprintln!("failed to fetch highest block from {peer_addr}: {e}, skipping");
                continue;
            }
        };

        if result
            .as_ref()
            .map_or(true, |(_, height)| peer_height > *height)
        {
            println!("new longest blockchain: {peer_height} blocks from {peer_addr}");
            result = Some((peer_addr, peer_height));
        }
    }

    result
}

pub async fn download_blockchain(peer_addr: &str, from_height: u64, to_height: u64) -> Result<()> {
    let conn = crate::NODES
        .get(peer_addr)
        .ok_or_else(|| anyhow!("no open connection to {peer_addr}"))?
        .clone();
    let mut conn = conn.lock().await;

    for height in from_height..to_height {
        let block = conn
            .request_expect(
                &Message::FetchBlock(height),
                DEFAULT_REQUEST_TIMEOUT,
                |m| match m {
                    Message::NewBlock(b) => Some(b),
                    _ => None,
                },
            )
            .await?;
        crate::BLOCKCHAIN.write().await.add_block(block)?;
    }
    Ok(())
}

pub async fn cleanup() {
    let mut interval = time::interval(time::Duration::from_secs(30));

    loop {
        interval.tick().await;
        println!("cleaning the mempool from old transactions");
        let mut blockchain = crate::BLOCKCHAIN.write().await;
        blockchain.cleanup_mempool();
    }
}

pub async fn save<P: AsRef<Path> + Clone>(path: P) {
    let mut interval = time::interval(time::Duration::from_secs(15));
    loop {
        interval.tick().await;
        println!("saving blockchain to drive...");
        let blockchain = crate::BLOCKCHAIN.write().await;
        if let Err(e) = blockchain.save_to_file(path.clone()) {
            eprintln!("blockchain checkpoint failed: {e}")
        }
    }
}

// TODO prevent broadcast loop when peers mutually know each other
pub async fn broadcast(message: &Message, label: &str) {
    // copy entries so mutex guards can be released after collect
    let peers = crate::NODES
        .iter()
        .map(|e| (e.key().clone(), e.value().clone()))
        .collect::<Vec<_>>();

    for (peer_addr, conn_wrapper) in peers {
        println!("{label} to {peer_addr}...");

        let mut conn = conn_wrapper.lock().await;
        if let Err(e) = conn.send(message).await {
            eprintln!("failed to {label} to {peer_addr}: {e}")
        }
    }
}
