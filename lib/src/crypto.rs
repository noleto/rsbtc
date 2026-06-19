use std::fmt::{Debug, Display};

use crate::sha256::Hash;
use ecdsa::{
    Signature as ECDSASignature, SigningKey, VerifyingKey,
    signature::{Signer, Verifier},
};
use k256::Secp256k1;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Signature(ECDSASignature<Secp256k1>);
#[derive(Serialize, Deserialize, Clone)]
pub struct PublicKey(VerifyingKey<Secp256k1>);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PrivateKey(#[serde(with = "signkey_serde")] pub SigningKey<Secp256k1>);

impl PrivateKey {
    pub fn new_key() -> Self {
        PrivateKey(SigningKey::random(&mut rand::thread_rng()))
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.0.verifying_key().clone())
    }
}

mod signkey_serde {
    use super::{Secp256k1, SigningKey};
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S>(key: &SigningKey<Secp256k1>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&key.to_bytes())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SigningKey<Secp256k1>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Vec::<u8>::deserialize(deserializer)?;
        Ok(SigningKey::from_slice(&bytes).or(Err(Error::custom(
            "unable to deserialize SigningKey from bytes representation",
        )))?)
    }
}

impl Signature {
    ///sign a crate::types::TransactionOutput from its Sha256 hash
    pub fn sign_output(output_hash: &Hash, private_key: &PrivateKey) -> Self {
        let signing_key = &private_key.0;
        let signature = signing_key.sign(&output_hash.as_bytes());
        Signature(signature)
    }

    ///verify a signature
    pub fn verify(&self, output_hash: &Hash, public_key: &PublicKey) -> bool {
        public_key
            .0
            .verify(&output_hash.as_bytes(), &self.0)
            .is_ok()
    }
}

impl Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SEC1(0x{})",
            hex::encode(self.0.to_encoded_point(true).as_bytes())
        )
    }
}

impl Debug for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
