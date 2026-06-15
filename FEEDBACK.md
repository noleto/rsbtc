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
```
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

## Page 224
- `mempool` field in the struct `Blockchain`:  `self.mempool.retain(|(_, tx)| block_txs.contains(&tx.hash()))` was introduced in `add_block` function but never declared before the page 241!
