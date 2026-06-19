use btclib::types::Transaction;
use btclib::utils::Saveable;
use std::env;
use std::fs::File;
use std::process::exit;
fn main() -> std::io::Result<()> {
    let path = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: tx_print <tx_file>");
        exit(1);
    });

    let file = File::open(path)?;
    let tx = Transaction::load(file)?;
    println!("{tx:#?}");
    Ok(())
}
