use crate::crypto::{PublicKey, Signature};
use crate::error::{BtcError, Result};
use crate::sha256::Hash;
use crate::util::MerkleRoot;
use crate::U256;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Blockchain {
    pub blocks: Vec<Block>,
    pub utxos: HashMap<Hash, TransactionOutput>,
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
    /// target
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

impl TransactionOutput {
    pub fn hash(&self) -> Hash {
        Hash::hash(self)
    }
}

impl Blockchain {
    pub fn new() -> Self {
        Blockchain {
            utxos: HashMap::new(),
            blocks: vec![],
        }
    }

    // block height
    pub fn block_height(&self) -> u64 {
        self.blocks.len() as u64
    }

    // Rebuild UTXO set from the blockchain
    pub fn rebuild_utxos(&mut self) {
        for block in &self.blocks {
            for tx in &block.transactions {
                for input in &tx.inputs {
                    self.utxos.remove(&input.prev_transaction_output_hash);
                }
                for output in tx.outputs.iter() {
                    self.utxos.insert(tx.hash(), output.clone());
                }
            }
        }
    }

    // try to add a new block to the blockchain,
    // return an error if it is not valid to insert this
    // block to this blockchain
    pub fn add_blocks(&mut self, block: Block) -> Result<()> {
        // check if the block is valid
        if self.blocks.is_empty() {
            // if this is the 1st block, check if the
            // block's previous block hash is all zeroes
            if block.header.prev_block_hash != Hash::zero() {
                print!("zero hash");
                return Err(BtcError::InvalidBlock);
            }
        } else {
            // if this is not the first block, check if the
            // block's previous block hash is the hash of the last block
            let last_block = self.blocks.last().unwrap();
            if block.header.prev_block_hash != last_block.hash() {
                print!("Previous hash is wrong");
                return Err(BtcError::InvalidBlock);
            }

            //check if the block's hash is less than the target
            if !block.header.hash().matches_target(block.header.target) {
                print!("Does not match target");
                return Err(BtcError::InvalidBlock);
            }

            //check if the block's merkle root is correct
            let calculated_merkle_root = MerkleRoot::calculate(&block.transactions);
            if calculated_merkle_root != block.header.merkle_root {
                print!("Invalid merkle root");
                return Err(BtcError::InvalidMerkleRoot);
            }

            //check if the block's timestamp is after the last block's timestamp
            if block.header.timestamp <= last_block.header.timestamp {
                return Err(BtcError::InvalidBlock);
            }

            //Verify all transactions in the block
            block.verify_transactions(self.block_height(), &self.utxos)?;
        }
        self.blocks.push(block);
        Ok(())
    }
}

impl Block {
    pub fn new(header: BlockHeader, transactions: Vec<Transaction>) -> Self {
        Block {
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
        let mut inputs: HashMap<Hash, TransactionOutput> = HashMap::new();

        // reject completely empty blocks
        if self.transactions.is_empty() {
            return Err(BtcError::InvalidTransaction);
        }

        // verify coinbase transaction
        self.verify_coinbase_transaction(predicted_block_height, utxos)?;

        for tx in self.transactions.iter().skip(1) {
            let mut input_value = 0;
            let mut output_value = 0;
            for input in &tx.inputs {
                let prev_output = utxos.get(&input.prev_transaction_output_hash);
                if prev_output.is_none() {
                    return Err(BtcError::InvalidTransaction);
                }

                let prev_output = prev_output.unwrap();
                // prevent same-block double-spending
                if inputs.contains_key(&input.prev_transaction_output_hash) {
                    return Err(BtcError::InvalidTransaction);
                }

                // check if the signature is valid
                if input
                    .signature
                    .verify(&input.prev_transaction_output_hash, &prev_output.pubkey)
                {
                    return Err(BtcError::InvalidSignature);
                }

                input_value += prev_output.value;
                inputs.insert(input.prev_transaction_output_hash, prev_output.clone());
            }

            for output in &tx.outputs {
                output_value += output.value;
            }

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
        let coinbase_tx = &self.transactions[0];
        if coinbase_tx.inputs.len() != 0 {
            return Err(BtcError::InvalidTransaction);
        }

        if coinbase_tx.outputs.len() == 0 {
            return Err(BtcError::InvalidTransaction);
        }

        let miner_fees = self.calculate_miner_fees(utxos).unwrap();
        let block_reward = crate::INITIAL_REWARD * 10u64.pow(8)
            / 2u64.pow((predicted_block_height / crate::HALVING_INTERVAL) as u32);

        let total_coinbase_outputs: u64 = coinbase_tx.outputs.iter().map(|o| o.value).sum();
        if total_coinbase_outputs != block_reward + miner_fees {
            return Err(BtcError::InvalidTransaction);
        }

        Ok(())
    }

    pub fn calculate_miner_fees(&self, utxos: &HashMap<Hash, TransactionOutput>) -> Result<u64> {
        let mut inputs: HashMap<Hash, TransactionOutput> = HashMap::new();
        let mut outputs: HashMap<Hash, TransactionOutput> = HashMap::new();

        // Check every transaction after coinbase
        for tx in self.transactions.iter().skip(1) {
            for input in &tx.inputs {
                // inputs do not contain the values of the outputs
                // so we need to match inputs to outputs
                let prev_output = utxos.get(&input.prev_transaction_output_hash);
                if prev_output.is_none() {
                    return Err(BtcError::InvalidTransaction);
                }

                let prev_output = prev_output.unwrap();
                if inputs.contains_key(&input.prev_transaction_output_hash) {
                    return Err(BtcError::InvalidTransaction);
                }

                inputs.insert(input.prev_transaction_output_hash, prev_output.clone());
            }
            for output in &tx.outputs {
                if outputs.contains_key(&output.hash()) {
                    return Err(BtcError::InvalidTransaction);
                }
                outputs.insert(output.hash(), output.clone());
            }
        }

        let input_value: u64 = inputs.values().map(|o| o.value).sum();
        let output_value: u64 = outputs.values().map(|o| o.value).sum();
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
        BlockHeader {
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
        Transaction { inputs, outputs }
    }

    pub fn hash(&self) -> Hash {
        Hash::hash(self)
    }
}
