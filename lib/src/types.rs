use std::collections::{HashMap, HashSet};

use crate::U256;
use crate::crypto::{PublicKey, Signature};
use crate::error::{BtcError, Result};
use crate::sha256::Hash;
use crate::util::MerkleRoot;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Blockchain {
    pub blocks: Vec<Block>,
    pub target: U256,
    pub utxos: HashMap<Hash, TransactionOutput>,
    #[serde(default, skip_serializing)]
    mempool: Vec<(DateTime<Utc>, Transaction)>,
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
    pub prev_block_hash: Hash,
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
    pub prev_transaction_output_hash: Hash,
    pub signature: Signature,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransactionOutput {
    pub value: u64,
    pub unique_id: Uuid,
    pub pubkey: PublicKey,
}

impl Blockchain {
    pub fn new() -> Self {
        Blockchain {
            blocks: vec![],
            target: crate::MIN_TARGET,
            mempool: vec![],
            utxos: HashMap::new(),
        }
    }

    /// Try to add a new block to the blockchain,
    /// return an error if it is not valid to insert this
    /// block to this blockchain
    pub fn add_block(&mut self, block: Block) -> Result<()> {
        //check if the block is valid
        if self.blocks.is_empty() {
            //this is first block, check if the block's previous block hash is all zeroes
            if block.header.prev_block_hash != Hash::ZERO {
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
        // TODO revisit that code
        let block_txs: HashSet<_> = block.transactions.iter().map(|tx| tx.hash()).collect();
        self.mempool
            .retain(|(_, tx)| block_txs.contains(&tx.hash()));

        self.blocks.push(block);

        self.try_adjust_target();
        Ok(())
    }

    pub fn try_adjust_target(&mut self) {
        if self.blocks.is_empty() {
            return;
        }

        if self.blocks.len() % crate::DIFFICULTY_UPDATE_INTERVAL as usize != 0 {
            return;
        }

        // measure the time it took to mine the last
        // {crate::DIFFICULTY_UPDATE_INTERVAL} blocks with chrono
        if let (Some(start_block), Some(end_block)) = (
            self.blocks
                .get(self.blocks.len() - crate::DIFFICULTY_UPDATE_INTERVAL as usize),
            self.blocks.last(),
        ) {
            let start_time = start_block.header.timestamp;
            let end_time = end_block.header.timestamp;
            let time_diff = end_time - start_time;

            let time_diff_secs = time_diff.num_seconds();
            // calculate the ideal number of seconds
            let target_secs = crate::IDEAL_BLOCK_TIME * crate::DIFFICULTY_UPDATE_INTERVAL;
            let new_target = self.target * (time_diff_secs as f64 / target_secs as f64) as u64;

            // clamp new_target to be within the range of
            // 4 * self.target and self.target / 4
            let new_target = new_target.clamp(self.target / 4, self.target * 4);

            // if the new target is more than the minimum target,
            // set it to the minimum target
            self.target = new_target.min(crate::MIN_TARGET);
        }
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
}

impl Block {
    pub fn new(header: BlockHeader, transactions: Vec<Transaction>) -> Self {
        Self {
            header,
            transactions,
        }
    }

    pub fn hash(&self) -> Hash {
        Hash::hash(self)
    }

    // Verify all transactions in the block
    pub fn verify_transactions(
        &self,
        predicted_block_height: u64,
        utxos: &HashMap<Hash, TransactionOutput>,
    ) -> Result<()> {
        // reject completely empty blocks
        if self.transactions.is_empty() {
            return Err(BtcError::InvalidTransaction);
        }

        // verify coinbase transaction
        self.verify_coinbase_transaction(predicted_block_height, utxos)?;

        let mut spent = HashSet::new();
        for tx in self.transactions.iter().skip(1) {
            let resolved = tx.resolve_inputs(utxos, &mut spent)?;
            for (input, previous_output) in resolved.iter() {
                if !input
                    .signature
                    .verify(&input.prev_transaction_output_hash, &previous_output.pubkey)
                {
                    return Err(BtcError::InvalidSignature);
                }
            }
            let input_value: u64 = resolved.iter().map(|(_, i)| i.value).sum();
            let output_value: u64 = tx.outputs.iter().map(|o| o.value).sum();

            // It is fine for output value to be less than input value
            // as the difference is the fee for the miner
            if input_value < output_value {
                return Err(BtcError::InvalidTransaction);
            }
        }

        Ok(())
    }

    pub fn verify_coinbase_transaction(
        &self,
        predicted_block_height: u64,
        utxos: &HashMap<Hash, TransactionOutput>,
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

    pub fn calculate_miner_fees(&self, utxos: &HashMap<Hash, TransactionOutput>) -> Result<u64> {
        let mut spent = HashSet::new();
        let (input_value, output_value) =
            self.transactions
                .iter()
                .skip(1)
                .try_fold((0u64, 0u64), |(ins, outs), tx| {
                    let resolved = tx.resolve_inputs(utxos, &mut spent)?;
                    let tx_in: u64 = resolved.iter().map(|(_, po)| po.value).sum();
                    let tx_out: u64 = tx.outputs.iter().map(|o| o.value).sum();
                    Ok((ins + tx_in, outs + tx_out))
                })?;
        Ok(input_value - output_value)
    }
}

impl BlockHeader {
    pub fn new(
        timestamp: DateTime<Utc>,
        nonce: u64,
        prev_block_hash: Hash,
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
}

impl Transaction {
    pub fn new(inputs: Vec<TransactionInput>, outputs: Vec<TransactionOutput>) -> Self {
        Self { inputs, outputs }
    }

    pub fn hash(&self) -> Hash {
        Hash::hash(self)
    }

    /// For each input, check for double-spend and look up the previous output in UTXOs
    fn resolve_inputs<'a>(
        &self,
        utxos: &'a HashMap<Hash, TransactionOutput>,
        spent: &mut HashSet<Hash>,
    ) -> Result<Vec<(&TransactionInput, &'a TransactionOutput)>> {
        self.inputs
            .iter()
            .map(|input| {
                if !spent.insert(input.prev_transaction_output_hash) {
                    return Err(BtcError::InvalidTransaction);
                }

                let previous_output = utxos
                    .get(&input.prev_transaction_output_hash)
                    .ok_or(BtcError::InvalidTransaction)?;
                Ok((input, previous_output))
            })
            .collect()
    }
}

impl TransactionInput {
    pub fn new(prev_transaction_output_hash: Hash, signature: Signature) -> Self {
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

    pub fn hash(&self) -> Hash {
        Hash::hash(self)
    }
}
