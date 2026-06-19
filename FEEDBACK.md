## Page 166
```rust
pub fn hash(&self) -> ! {
    unimplemented!() // should be todo!()
}
```
Rust doc: The difference between unimplemented and todo! is that while todo! conveys an intent of implementing the functionality later and the message is “not yet implemented”, unimplemented! makes no such claims. Its message is “not implemented”.

## Page 177
With the new version of crate uint (0.10.0), no more support from converting from an array (see https://github.com/paritytech/parity-common/pull/859):
```
let hash = digest(&serialized);
let hash_bytes = hex::decode(hash).unwrap();
let hash_array: [u8; 32] = hash_bytes.as_slice()
    .try_into()
    .unwrap();
Hash(U256::from(hash_array)) // removed from the api
```

Replacement code (that uses internally the new `from_big_endian(&bytes)`)
```
let hash = digest(&serialized);
Hash(U256::from_str_radix(&hash, 16).expect("Cannot decode the sha256 digest!"))
```


## Page 187
- Requires rand = {version = "0.8.5"} as new version "0.10.1" dropped support for `rand::thread_rng()` and ecdsa = { version = "0.16.9" } still requires it!
- hash.rs => should bre read "sha256.rs"

## Page 207
- The function `rebuild_utxos` has a logic issue: The key used for insertion should be `output.hash()`, not `tx.hash()`. Since `TransactionOutput` has a `unique_id: Uuid` field, each output produces a distinct hash. The `prev_transaction_output_hash` on inputs is meant to reference a specific output's hash, not the parent transaction's hash.
```
for output in &tx.outputs {
    self.utxos.insert(output.hash(), output.clone());
//                    ^^^^^^^^^^^^^ was transaction.hash()
}
```

## Page 209
- Misleading phrase "If you run cargo check now, you should have not just no errors, but also no warnings": The function `verify_transactions` was never defined before that point (it comes just after that phrase)
- Method `block_height` is used by only defined in page 226!

## Page 218
- Missing `mempool` field in the struct `Blockchain`:  `self.mempool.retain(|(_, tx)| { !block_transactions.contains(&tx.hash()) });` was introduced in `add_block` function but never declared before the page 227, and in the meantime authors ask to run `cargo check` in page 224, sic "you should be getting no warnings and no compilation errors whatsoever".


## Page 239
- typo: "This method() includes the index of each element with it..." => "This method includes..."


## Page 

- Display bug on hash (strips leading zeros): On page 180, the Display trait for hash is implemented as:
```
impl fmt::Display for Hash {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
  write!(f, "{:x}", self.0)
  }
}
```
{:x}` formats a `U256` as hex but **strips leading zeros**. Since a mined hash satisfies `hash <= target`, it has small numeric value — which means leading zero bytes — and those zeros are exactly what proof-of-work produces. The formatter then drops them, so a valid mined hash *looks* like it starts with `25c3...` when it really starts with `0000...25c3`.

The fix is:
```
impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:064x}", self.0)
    }
}
```
The `0` flag means pad with zeros, `64` is the width (32 bytes × 2 hex chars)
