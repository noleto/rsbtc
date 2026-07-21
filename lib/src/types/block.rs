use std::collections::HashMap;

use super::{Transaction, TransactionOutput};
use crate::U256;
use crate::error::{BtcError, Result};
use crate::sha256::{BlockHash, Hash, TxOutputHash};
use crate::utils::{AutoSaveable, MerkleRoot};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

impl AutoSaveable for Block {}

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
        utxos: &HashMap<TxOutputHash, TransactionOutput>,
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
        utxos: &HashMap<TxOutputHash, TransactionOutput>,
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
        let block_reward = Self::block_reward(predicted_block_height);

        let total_coinbase_value: u64 = coinbase_tx.outputs.iter().map(|o| o.value).sum();
        if total_coinbase_value != block_reward + miner_fees {
            return Err(BtcError::InvalidTransaction);
        }
        Ok(())
    }

    pub fn calculate_miner_fees(
        &self,
        utxos: &HashMap<TxOutputHash, TransactionOutput>,
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

    pub fn block_reward(block_height: u64) -> u64 {
        let block_reward = crate::INITIAL_REWARD * 10u64.pow(8)
            / 2u64.pow((block_height / crate::HALVING_INTERVAL) as u32);
        return block_reward;
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
