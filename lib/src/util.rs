use serde::{Deserialize, Serialize};

use crate::sha256::Hash;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MerkleRoot(Hash);
