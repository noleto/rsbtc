use btclib::crypto::PrivateKey;
use btclib::types::{Transaction, TransactionOutput};
use btclib::utils::Saveable;
use std::env;
use std::process::exit;
use uuid::Uuid;

fn main() -> std::io::Result<()> {
    let path = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: tx_gen <block_file>");
        exit(1);
    });

    let private_key = PrivateKey::new_key();
    let tx = Transaction::new(
        vec![],
        vec![TransactionOutput {
            unique_id: Uuid::new_v4(),
            value: btclib::INITIAL_REWARD * 10u64.pow(8),
            pubkey: private_key.public_key(),
        }],
    );

    tx.save_to_file(path)?;

    Ok(())
}
