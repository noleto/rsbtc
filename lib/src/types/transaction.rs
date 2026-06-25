use std::collections::{HashMap, HashSet};

use crate::crypto::{PublicKey, Signature};
use crate::error::{BtcError, Result};
use crate::sha256::{Hash, Txid, UtxoHash};
use crate::utils::AutoSaveable;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

impl Transaction {
    pub fn new(inputs: Vec<TransactionInput>, outputs: Vec<TransactionOutput>) -> Self {
        Self { inputs, outputs }
    }

    pub fn hash(&self) -> Txid {
        Txid(Hash::hash(self))
    }

    /// Resolves each input to its previous [`TransactionOutput`] from the UTXO set.
    ///
    /// For each input, this function:
    /// - Detects same-transaction double-spends (two inputs referencing the same UTXO)
    /// - Verifies the referenced UTXO exists in the provided UTXO set
    ///
    /// Returns a paired list of `(input, previous_output)` so callers have both sides
    /// without needing to re-query the UTXO set.
    ///
    /// # Errors
    /// Returns [`BtcError::InvalidTransaction`] if any input is duplicated or references
    /// a UTXO that does not exist in `utxos`.
    pub fn resolve_inputs<'a>(
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

    /// Validates that inputs cover outputs and returns their summed values.
    ///
    /// In Bitcoin, transactions have no explicit fee field — the miner fee is the implicit
    /// difference between the total input value and the total output value. Outputs are
    /// allowed to be less than inputs (the difference goes to the miner), but outputs
    /// must never exceed inputs.
    ///
    /// # Returns
    /// A tuple of `(input_value, output_value)` in satoshis, both already validated.
    /// The caller can compute the miner fee as `input_value - output_value`.
    ///
    /// # Errors
    /// Returns [`BtcError::InvalidTransaction`] if the total output value exceeds
    /// the total input value.
    pub fn verified_spend(
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

impl AutoSaveable for Transaction {}
