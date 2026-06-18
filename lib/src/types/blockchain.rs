use std::collections::HashMap;

use super::{Block, Transaction, TransactionOutput};
use crate::U256;
use crate::error::{BtcError, Result};
use crate::sha256::{Hash, Txid, UtxoHash};
use crate::utils::MerkleRoot;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Blockchain {
    blocks: Vec<Block>,
    target: U256,
    // confirmed, spendable outputs
    utxos: HashMap<UtxoHash, TransactionOutput>,
    #[serde(default, skip_serializing, skip_deserializing)]
    mempool: HashMap<Txid, MempoolEntry>,
    #[serde(default, skip_serializing, skip_deserializing)]
    // tracks UTXOs primed for spending by a transaction in mempool
    // utxo hash → mempool tx spending it
    pending_spends: HashMap<UtxoHash, Txid>,
}

#[derive(Clone, Debug)]
pub struct MempoolEntry {
    time_added: DateTime<Utc>,
    miner_fees: u64,
    transaction: Transaction,
}

impl Blockchain {
    /// Creates a new empty blockchain with the minimum proof-of-work target.
    pub fn new() -> Self {
        Blockchain {
            blocks: vec![],
            target: crate::MIN_TARGET,
            mempool: HashMap::new(),
            utxos: HashMap::new(),
            pending_spends: HashMap::new(),
        }
    }

    /// Validates and appends a block to the chain.
    ///
    /// Enforces the following consensus rules:
    /// - The genesis block must reference an all-zero previous block hash
    /// - Every subsequent block must reference the hash of the current chain tip
    /// - The block header hash must satisfy the proof-of-work target
    /// - The merkle root must match the block's transaction set
    /// - The block timestamp must be strictly after the previous block's timestamp
    /// - All transactions must be valid against the current UTXO set
    ///
    /// On success, confirmed transactions are removed from the mempool, the block
    /// is appended to the chain, and the difficulty target is adjusted if due.
    ///
    /// # Errors
    /// Returns [`BtcError::InvalidBlock`] or [`BtcError::InvalidMerkleRoot`] if any
    /// consensus rule is violated.
    pub fn add_block(&mut self, block: Block) -> Result<()> {
        //check if the block is valid
        if self.blocks.is_empty() {
            //this is first block, check if the block's previous block hash is all zeroes
            if block.header.prev_block_hash.0 != Hash::ZERO {
                return Err(BtcError::InvalidBlock);
            }
        } else {
            // if this is not the first block, check if the
            // block's previous block is the hash of the last block
            let last_block = self.blocks.last().expect("blockchain should not be empty");
            if block.header.prev_block_hash != last_block.hash() {
                return Err(BtcError::InvalidBlock);
            }

            // check if the block's hash is less than the target
            if !block.header.hash().matches_target(block.header.target) {
                return Err(BtcError::InvalidBlock);
            }

            // check if the block's merkle root is correct
            let calculated_merkle_root = MerkleRoot::calculate(&block.transactions);
            if calculated_merkle_root != block.header.merkle_root {
                return Err(BtcError::InvalidMerkleRoot);
            }

            // check if the block's timestamp is after the
            // last block's timestamp
            if block.header.timestamp <= last_block.header.timestamp {
                return Err(BtcError::InvalidBlock);
            }

            // Verify all transactions in the block
            block.verify_transactions(self.block_height(), &self.utxos)?;
        }
        // Remove transactions from mempool that are now in the block
        for tx in &block.transactions {
            self.mempool.remove(&tx.hash());
        }

        self.blocks.push(block);

        self.try_adjust_target();
        Ok(())
    }

    /// Adjusts the proof-of-work target every [`DIFFICULTY_UPDATE_INTERVAL`] blocks.
    ///
    /// Compares the actual time taken to mine the last interval of blocks against the
    /// ideal time (`DIFFICULTY_UPDATE_INTERVAL × IDEAL_BLOCK_TIME`). The target is
    /// scaled proportionally, then clamped to `[target/4, target×4]` to prevent
    /// runaway difficulty swings, and further capped at [`MIN_TARGET`].
    ///
    /// This is a simplified version of Bitcoin's difficulty adjustment, which recalculates every 2016 blocks
    /// aiming for a 10-minute average block time.
    ///
    /// No-ops if the chain length is not a multiple of [`DIFFICULTY_UPDATE_INTERVAL`].
    pub fn try_adjust_target(&mut self) {
        if self.blocks.is_empty()
            || self.blocks.len() % crate::DIFFICULTY_UPDATE_INTERVAL as usize != 0
        {
            return;
        }

        // measure the time it took to mine the last
        // {crate::DIFFICULTY_UPDATE_INTERVAL} blocks with chrono
        let start_block =
            &self.blocks[self.blocks.len() - crate::DIFFICULTY_UPDATE_INTERVAL as usize];
        let end_block = self
            .blocks
            .last()
            .expect("checked non-empty above, should never happen");

        let time_diff_secs =
            (end_block.header.timestamp - start_block.header.timestamp).num_seconds();
        // calculate the ideal number of seconds
        let target_secs = crate::IDEAL_BLOCK_TIME * crate::DIFFICULTY_UPDATE_INTERVAL;
        // clamp new_target to be within the range of
        // 4 * self.target and self.target / 4
        let new_target = (self.target * (time_diff_secs as f64 / target_secs as f64) as u64)
            .clamp(self.target / 4, self.target * 4);

        // if the new target is less than the minimum target,
        // set it to the minimum target
        self.target = new_target.min(crate::MIN_TARGET);
    }

    /// Rebuilds the UTXO set by replaying every block from genesis.
    ///
    /// For each transaction, all referenced inputs are removed from the UTXO set
    /// (spent) and all outputs are inserted (unspent). This is used to reconstruct
    /// the UTXO set after deserialization, since `utxos` is not persisted.
    pub fn rebuild_utxos(&mut self) {
        for block in &self.blocks {
            for tx in &block.transactions {
                for input in &tx.inputs {
                    self.utxos.remove(&input.prev_transaction_output_hash);
                }
                for output in &tx.outputs {
                    self.utxos.insert(output.hash(), output.clone());
                }
            }
        }
    }

    // block height
    pub fn block_height(&self) -> u64 {
        self.blocks.len() as u64
    }

    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.blocks.iter()
    }

    pub fn target(&self) -> U256 {
        self.target
    }

    pub fn utxos(&self) -> &HashMap<UtxoHash, TransactionOutput> {
        &self.utxos
    }

    pub fn mempool(&self) -> &HashMap<Txid, MempoolEntry> {
        &self.mempool
    }

    /// Validates a transaction and admits it into the mempool.
    ///
    /// Enforces consensus and mempool policy in order:
    /// 1. All inputs must reference existing UTXOs with valid signatures
    /// 2. Total output value must not exceed total input value
    /// 3. Replace-By-Fee (RBF, BIP-125, simplified): if any input conflicts with a
    ///    pending mempool transaction, the incoming tx must pay strictly higher fees
    ///    to evict it — otherwise the incoming tx is dropped
    ///
    /// On successful admission, conflicting transactions are evicted, their
    /// non-conflicting UTXO claims are released, and the new transaction is
    /// tracked with its implicit miner fee.
    ///
    /// # Errors
    /// - [`BtcError::InvalidTransaction`] — input validation failed
    /// - [`BtcError::InvalidSignature`] — a signature did not verify
    /// - [`BtcError::TransactionDropped`] — lost the RBF fee competition
    pub fn add_to_mempool(&mut self, tx: Transaction) -> Result<()> {
        // consensus: all inputs must reference existing UTXOs and carry valid signatures
        let resolved = tx.resolve_inputs(&self.utxos())?;

        // consensus: sum of outputs must not exceed sum of inputs;
        // the difference is the implicit miner fee (no explicit fee field in Bitcoin)
        let (sum_inputs, sum_outputs) = tx.verified_spend(resolved)?;
        let miner_fees = sum_inputs - sum_outputs;

        // a new tx may evict a conflicting
        // mempool tx only if it pays strictly higher fees; otherwise it is dropped
        // this is simplification of real Bitcoin Replace-By-Fee policy (RBF, BIP-125) because
        // we don't check if the tx being replaced opted-in to allowing replacement
        // of itself if any of its inputs have an nSequence number less than (0xffffffff - 1)
        for input in &tx.inputs {
            if let Some(tracked_hash) = self.pending_spends.get(&input.prev_transaction_output_hash)
            {
                let entry = self
                    .mempool
                    .get(tracked_hash)
                    .expect("pending_spends must always reference a live mempool entry");
                if miner_fees <= entry.miner_fees {
                    return Err(BtcError::TransactionDropped(
                        "a pending transaction with equal or higher miner fees already spends one or more of the same UTXOs".to_string(),
                    ));
                }
            }
        }

        // RBF eviction: remove every conflicting mempool tx and claim their UTXOs
        let mut txs_evicted = vec![];
        let new_tx_hash = tx.hash();
        for input in &tx.inputs {
            self.pending_spends
                .entry(input.prev_transaction_output_hash)
                .and_modify(|tracked_hash| {
                    if let Some(entry) = self.mempool.remove(tracked_hash) {
                        txs_evicted.push(entry.transaction);
                    }
                    *tracked_hash = tx.hash();
                })
                .or_insert(new_tx_hash);
        }

        // release the non-conflicting inputs of each evicted tx so they become
        // spendable again by future transactions
        for tx_evicted in txs_evicted {
            for evicted_input in &tx_evicted.inputs {
                let hash = &evicted_input.prev_transaction_output_hash;
                if self.pending_spends.get(hash) != Some(&new_tx_hash) {
                    self.pending_spends.remove(hash);
                }
            }
        }

        // admit the transaction into the mempool where it awaits block inclusion
        self.mempool
            .insert(tx.hash(), MempoolEntry::new(Utc::now(), miner_fees, tx));

        Ok(())
    }

    /// Returns mempool transactions sorted by miner fee in descending order.
    ///
    /// Miners should pick from the front of the iterator to maximise revenue.
    pub fn sort_mempool(&mut self) -> impl Iterator<Item = Transaction> {
        let mut pending_txs: Vec<(&Transaction, u64)> = self
            .mempool
            .values()
            .map(|mempool_entry| (&mempool_entry.transaction, mempool_entry.miner_fees))
            .collect();
        pending_txs.sort_by_key(|(_, miner_fees)| std::cmp::Reverse(*miner_fees));
        pending_txs.into_iter().map(|(tx, _)| tx.clone())
    }

    /// Evicts transactions that have been pending longer than [`MAX_MEMPOOL_TRANSACTION_AGE`].
    ///
    /// Stale transactions are removed from the mempool and their UTXO claims are
    /// released from `pending_spends`, making those UTXOs available for new transactions.
    ///
    /// This mirrors Bitcoin Core's mempool expiry policy (default: 336 hours => 14 days),
    /// which prevents the mempool from growing unboundedly with unconfirmed transactions.
    pub fn cleanup_mempool(&mut self) {
        let now = Utc::now();
        let evicted_txs = self
            .mempool
            .extract_if(|_k, v| {
                now - v.time_added
                    > chrono::Duration::seconds(crate::MAX_MEMPOOL_TRANSACTION_AGE as i64)
            })
            .map(|(_k, v)| v.transaction);

        // Untrack all of the UTXOs eventually tracked by evicted txs inputs
        for evicted_tx in evicted_txs {
            for input in evicted_tx.inputs {
                self.pending_spends
                    .remove(&input.prev_transaction_output_hash);
            }
        }
    }
}

impl MempoolEntry {
    pub fn new(time_added: DateTime<Utc>, miner_fees: u64, transaction: Transaction) -> Self {
        Self {
            time_added,
            miner_fees,
            transaction,
        }
    }
}
