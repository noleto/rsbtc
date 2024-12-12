use serde::{Deserialize, Serialize};

use crate::{sha256::Hash, types::Transaction};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MerkleRoot(Hash);

impl MerkleRoot {
    pub fn calculate(txs: &Vec<Transaction>) -> MerkleRoot {
        unimplemented!()
    }
}
