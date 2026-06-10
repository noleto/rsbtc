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
