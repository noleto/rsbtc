use std::fmt::{self, Debug, Display};

use crate::{sha256::Hash, types::Transaction};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct MerkleRoot(Hash);

impl MerkleRoot {
    // calculate the merkle root of a block's transactions
    pub fn calculate(transactions: &[Transaction]) -> MerkleRoot {
        let mut layer: Vec<Hash> = transactions.iter().map(|tx| Hash::hash(tx)).collect();
        while layer.len() > 1 {
            layer = layer
                .chunks(2)
                .map(|pair| Hash::hash(&[&pair[0], &pair.get(1).unwrap_or(&pair[0])]))
                .collect()
        }

        assert_eq!(layer.len(), 1, "MerkelRoot should contain a single hash");
        MerkleRoot(layer[0])
    }
}

impl Display for MerkleRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl Debug for MerkleRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}
