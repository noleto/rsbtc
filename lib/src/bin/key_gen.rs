use std::env;

use btclib::crypto::PrivateKey;
use btclib::utils::Saveable;

fn main() -> std::io::Result<()> {
    let name = env::args()
        .nth(1)
        .expect("should provide a name as first argument");
    let priv_key = PrivateKey::new_key();
    let pub_key = priv_key.public_key();

    priv_key.save_to_file(format!("{}.priv.cbor", name))?;
    pub_key.save_to_file(format!("{}.pub.pem", name))?;
    Ok(())
}
