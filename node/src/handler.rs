use btclib::network::Connection;
use btclib::network::Message;
use btclib::network::Message::*;
use btclib::network::NetworkError;
use btclib::sha256::BlockHash;

use crate::util;

pub async fn handle_connection(mut conn: Connection) -> Result<(), NetworkError> {
    loop {
        // read a message from the established connection
        let message = match conn.receive().await {
            Ok(m) => m,
            // Treat client EOF as a clean disconnect, not an error.
            Err(NetworkError::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(()); // peer hung up — normal
            }
            Err(e) => return Err(e),
        };
        match message {
            UTXOs(_)
            | Template(_)
            | BlockCount(_)
            | TemplateValidity(_)
            | NodeList(_)
            | TemplateNotAvailable()
            | BlockNotFound()
            | Unsupported() => {
                println!("received a message that cannot be handled: {message}");
                conn.send(&Message::Unsupported()).await?
            }
            FetchBlock(height) => {
                println!("received FetchBlock request for height: {message}");
                let blockchain = crate::BLOCKCHAIN.read().await;
                match blockchain.blocks().nth(height as usize).cloned() {
                    Some(block) => conn.send(&NewBlock(block)).await?,
                    None => conn.send(&BlockNotFound()).await?,
                }
            }
            DiscoverNodes => {
                println!("received DiscoverNodes request");
                let nodes = crate::NODES
                    .iter()
                    .map(|kv| kv.key().clone())
                    .collect::<Vec<_>>();
                conn.send(&Message::NodeList(nodes)).await?
            }
            AskBlockCount => {
                println!("received AskBlockCount request");
                let blockchain = crate::BLOCKCHAIN.read().await;
                let count = blockchain.block_height();
                conn.send(&Message::BlockCount(count)).await?
            }
            FetchUTXOs(key) => {
                println!("received FetchUTXOs request for key: {key}");
                let blockchain = crate::BLOCKCHAIN.read().await;
                let utxos: Vec<_> = blockchain
                    .scan_utxos(key)
                    .map(|(txout, primed)| (txout.clone(), primed))
                    .collect();
                conn.send(&UTXOs(utxos)).await?
            }
            NewBlock(block) => {
                let block_hash = &block.hash();
                println!("received NewBlock request, block hash: {}", block_hash);
                let mut blockchain = crate::BLOCKCHAIN.write().await;
                if blockchain.add_block(block).is_err() {
                    println!("block {} rejected", block_hash);
                }
            }
            NewTransaction(tx) => {
                let tx_hash = &tx.hash();
                println!("received NewTransaction request, TXID: {}", tx_hash);
                let mut blockchain = crate::BLOCKCHAIN.write().await;
                if blockchain.add_to_mempool(tx).is_err() {
                    println!("transaction rejected: {}", tx_hash);
                }
                //TODO relay tx to other nodes, while preveing notification loops
            }
            ValidateTemplate(block) => {
                println!(
                    "received ValidateTemplate request, block template hash: {}",
                    block.hash()
                );
                let blockchain = crate::BLOCKCHAIN.read().await;
                //invalid if it is no longer pointing to the top of the blockchain
                let chain_tip = blockchain.chain_tip().unwrap_or_else(|| BlockHash::ZERO);
                let status = block.header.prev_block_hash == chain_tip;
                conn.send(&TemplateValidity(status)).await?
            }
            SubmitTemplate(block) => {
                println!(
                    "received SubmitTemplate request, block template hash: {}",
                    block.hash()
                );
                let accepted = {
                    let mut blockchain = crate::BLOCKCHAIN.write().await;
                    match blockchain.add_block(block.clone()) {
                        Ok(()) => {
                            println!("block accepted");
                            true
                        }
                        Err(e) => {
                            println!("block rejected: {e}");
                            false
                        }
                    }
                };
                if accepted {
                    util::broadcast(&Message::NewBlock(block), "broadcasting block").await;
                }
            }
            SubmitTransaction(tx) => {
                println!("received SubmitTransaction request, TXID: {}", tx.hash());

                let accepted = {
                    let mut blockchain = crate::BLOCKCHAIN.write().await;
                    match blockchain.add_to_mempool(tx.clone()) {
                        Ok(()) => {
                            println!("tx added transaction to mempool");
                            true
                        }
                        Err(e) => {
                            println!("transaction rejected: {e}");
                            false
                        }
                    }
                };
                if accepted {
                    util::broadcast(&Message::NewTransaction(tx), "broadcasting transaction").await;
                }
            }
            FetchTemplate(pubkey) => {
                println!("received FetchTemplate request, pubkey: {pubkey}");
                let blockchain = crate::BLOCKCHAIN.read().await;
                match blockchain.block_template(pubkey) {
                    Ok(block_template) => conn.send(&Template(block_template)).await?,
                    Err(e) => {
                        eprintln!("error while preparing block template: {e}");
                        conn.send(&TemplateNotAvailable()).await?
                    }
                }
            }
        }
    }
}
