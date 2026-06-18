use std::collections::{HashMap, HashSet};

use crate::U256;
use crate::crypto::{PublicKey, Signature};
use crate::error::{BtcError, Result};
use crate::sha256::{BlockHash, Hash, Txid, UtxoHash};
use crate::util::MerkleRoot;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlockHeader {
    /// Timestamp of the block
    pub timestamp: DateTime<Utc>,
    /// Nonce used to mine the block
    pub nonce: u64,
    /// Hash of the previous block
    pub prev_block_hash: BlockHash,
    /// Merkle root of the block's transactions
    pub merkle_root: MerkleRoot,
    /// Target
    pub target: U256,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Transaction {
    pub inputs: Vec<TransactionInput>,
    pub outputs: Vec<TransactionOutput>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransactionInput {
    pub prev_transaction_output_hash: UtxoHash,
    pub signature: Signature,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransactionOutput {
    pub value: u64,
    pub unique_id: Uuid,
    pub pubkey: PublicKey,
}

#[derive(Clone, Debug)]
pub struct MempoolEntry {
    time_added: DateTime<Utc>,
    miner_fees: u64,
    transaction: Transaction,
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

impl Blockchain {
    pub fn new() -> Self {
        Blockchain {
            blocks: vec![],
            target: crate::MIN_TARGET,
            mempool: HashMap::new(),
            utxos: HashMap::new(),
            pending_spends: HashMap::new(),
        }
    }

    /// Try to add a new block to the blockchain,
    /// return an error if it is not valid to insert this
    /// block to this blockchain
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

    // Rebuild UTXO set from the blockchain
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

    pub fn sort_mempool(&mut self) -> impl Iterator<Item = Transaction> {
        let mut pending_txs: Vec<(&Transaction, u64)> = self
            .mempool
            .values()
            .into_iter()
            .map(|mempool_entry| (&mempool_entry.transaction, mempool_entry.miner_fees))
            .collect();
        pending_txs.sort_by_key(|(_, miner_fees)| *miner_fees);
        pending_txs.into_iter().map(|(tx, _)| tx.clone())
    }

    /// Cleanup mempool - remove transactions older than
    /// MAX_MEMPOOL_TRANSACTION_AGE
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

impl Block {
    pub fn new(header: BlockHeader, transactions: Vec<Transaction>) -> Self {
        Self {
            header,
            transactions,
        }
    }

    pub fn hash(&self) -> BlockHash {
        BlockHash(Hash::hash(self))
    }

    // Verify all transactions in the block
    pub fn verify_transactions(
        &self,
        predicted_block_height: u64,
        utxos: &HashMap<UtxoHash, TransactionOutput>,
    ) -> Result<()> {
        // reject completely empty blocks
        if self.transactions.is_empty() {
            return Err(BtcError::InvalidTransaction);
        }

        // verify coinbase transaction
        self.verify_coinbase_transaction(predicted_block_height, utxos)?;

        for tx in self.transactions.iter().skip(1) {
            let resolved = tx.resolve_inputs(utxos)?;
            for (input, previous_output) in resolved.iter() {
                if !input.signature.verify(
                    &input.prev_transaction_output_hash.0,
                    &previous_output.pubkey,
                ) {
                    return Err(BtcError::InvalidSignature);
                }
            }

            _ = tx.verified_spend(resolved)?;
        }

        Ok(())
    }

    pub fn verify_coinbase_transaction(
        &self,
        predicted_block_height: u64,
        utxos: &HashMap<UtxoHash, TransactionOutput>,
    ) -> Result<()> {
        // coinbase tx is the first transaction in the block
        let coinbase_tx = &self
            .transactions
            .get(0)
            .ok_or(BtcError::InvalidTransaction)?;

        // coinbase tx is created Ex nihilo so should have no inputs
        if !coinbase_tx.inputs.is_empty() {
            return Err(BtcError::InvalidTransaction);
        }

        // coinbase shoud send minted coins to someone
        if coinbase_tx.outputs.is_empty() {
            return Err(BtcError::InvalidTransaction);
        }

        let miner_fees = self.calculate_miner_fees(utxos)?;
        let block_reward = crate::INITIAL_REWARD * 10u64.pow(8)
            / 2u64.pow((predicted_block_height / crate::HALVING_INTERVAL) as u32);

        let total_coinbase_value: u64 = coinbase_tx.outputs.iter().map(|o| o.value).sum();
        if total_coinbase_value != block_reward + miner_fees {
            return Err(BtcError::InvalidTransaction);
        }
        Ok(())
    }

    pub fn calculate_miner_fees(
        &self,
        utxos: &HashMap<UtxoHash, TransactionOutput>,
    ) -> Result<u64> {
        let (input_value, output_value) =
            self.transactions
                .iter()
                .skip(1)
                .try_fold((0u64, 0u64), |(ins, outs), tx| {
                    let resolved = tx.resolve_inputs(utxos)?;
                    let (tx_in, tx_out) = tx.verified_spend(resolved)?;
                    Ok((ins + tx_in, outs + tx_out))
                })?;
        Ok(input_value - output_value)
    }
}

impl BlockHeader {
    pub fn new(
        timestamp: DateTime<Utc>,
        nonce: u64,
        prev_block_hash: BlockHash,
        merkle_root: MerkleRoot,
        target: U256,
    ) -> Self {
        Self {
            timestamp,
            nonce,
            prev_block_hash,
            merkle_root,
            target,
        }
    }

    pub fn hash(&self) -> Hash {
        Hash::hash(self)
    }

    pub fn mine(&mut self, steps: usize) -> bool {
        // if the block already matches target, return early
        if self.hash().matches_target(self.target) {
            return true;
        }

        for _ in 0..steps {
            if let Some(new_nonce) = self.nonce.checked_add(1) {
                self.nonce = new_nonce;
            } else {
                self.nonce = 0;
                self.timestamp = Utc::now();
            }

            if self.hash().matches_target(self.target) {
                return true;
            }
        }
        false
    }
}

impl Transaction {
    pub fn new(inputs: Vec<TransactionInput>, outputs: Vec<TransactionOutput>) -> Self {
        Self { inputs, outputs }
    }

    pub fn hash(&self) -> Txid {
        Txid(Hash::hash(self))
    }

    /// For each input, check for double-spend and look up the previous output in UTXOs
    fn resolve_inputs<'a>(
        &self,
        utxos: &'a HashMap<UtxoHash, TransactionOutput>,
    ) -> Result<Vec<(&TransactionInput, &'a TransactionOutput)>> {
        //Track already "spent" inputs
        let mut known_inputs = HashSet::new();
        self.inputs
            .iter()
            .map(|input| {
                if !known_inputs.insert(input.prev_transaction_output_hash) {
                    return Err(BtcError::InvalidTransaction);
                }

                let previous_output = utxos
                    .get(&input.prev_transaction_output_hash)
                    .ok_or(BtcError::InvalidTransaction)?;
                Ok((input, previous_output))
            })
            .collect()
    }

    fn verified_spend(
        &self,
        resolved_inputs: Vec<(&TransactionInput, &TransactionOutput)>,
    ) -> Result<(u64, u64)> {
        let input_value: u64 = resolved_inputs.iter().map(|(_, i)| i.value).sum();
        let output_value: u64 = self.outputs.iter().map(|o| o.value).sum();

        // It is fine for output value to be less than input value
        // as the difference is the fee for the miner
        if input_value < output_value {
            return Err(BtcError::InvalidTransaction);
        }
        Ok((input_value, output_value))
    }
}

impl TransactionInput {
    pub fn new(prev_transaction_output_hash: UtxoHash, signature: Signature) -> Self {
        Self {
            prev_transaction_output_hash,
            signature,
        }
    }
}

impl TransactionOutput {
    pub fn new(value: u64, unique_id: Uuid, pubkey: PublicKey) -> Self {
        Self {
            value,
            unique_id,
            pubkey,
        }
    }

    pub fn hash(&self) -> UtxoHash {
        UtxoHash(Hash::hash(self))
    }
}
