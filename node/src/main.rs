use std::{path::Path, sync::Arc};

use anyhow::Result;
use argh::FromArgs;
use btclib::{network::Connection, types::Blockchain};
use dashmap::DashMap;
use static_init::dynamic;
use tokio::{
    net::TcpListener,
    sync::{Mutex, RwLock},
};

mod handler;
mod util;

#[dynamic]
pub static BLOCKCHAIN: RwLock<Blockchain> = RwLock::new(Blockchain::new());

#[dynamic]
pub static NODES: DashMap<String, Arc<Mutex<Connection>>> = DashMap::new();

#[derive(FromArgs)]
/// A toy blockchain node
struct Args {
    #[argh(option, default = "9000")]
    ///port number
    port: u16,
    #[argh(option, default = "String::from(\"./blockchain.cbor\")")]
    /// blockchain file location
    blockchain_file: String,
    #[argh(positional)]
    /// addresses of initial nodes
    nodes: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args: Args = argh::from_env();
    let port = args.port;
    let blockchain_file = args.blockchain_file;
    let nodes = args.nodes;

    // Check if the blockchain_file exists
    if Path::new(&blockchain_file).exists() {
        util::load_blockchain(&blockchain_file).await?;
    } else {
        println!("blockchain file does not exist!");
        util::populate_connections(&nodes).await?;
        println!("total amount of known nodes: {}", NODES.len());
        if nodes.is_empty() {
            println!("no initial nodes provided, starting as a seed node");
        } else {
            //fetching from other nodes
            if let Some((peer_addr, target_height)) = util::find_longest_chain_node().await {
                // request the blockchain from the node with the longest blockchain
                if let Err(e) = util::download_blockchain(&peer_addr, 0u64, target_height).await {
                    println!("failed to sync block from peers: {e}");
                }
                // recalculate utxos
                {
                    let mut blockchain = BLOCKCHAIN.write().await;
                    blockchain.rebuild_utxos();
                }
                // try to adjust difficulty
                {
                    let mut blockchain = BLOCKCHAIN.write().await;
                    blockchain.try_adjust_target();
                }
            }
        }
    }

    // Start the TCP listener on 0.0.0.0:port
    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).await?;
    println!("Listening on {addr}");
    // start a task to periodically cleanup the mempool
    // normally, you would want to keep and join the handle
    tokio::spawn(util::cleanup());
    // and a task to periodically save the blockchain
    tokio::spawn(util::save(blockchain_file.clone()));
    loop {
        let (socket, addr) = listener.accept().await?;
        tokio::spawn(handler::handle_connection(Connection::from_stream(
            addr.to_string(),
            socket,
        )));
    }
}
