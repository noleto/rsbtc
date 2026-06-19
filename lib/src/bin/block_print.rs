use btclib::types::Block;
use btclib::utils::Saveable;
use std::env;
use std::fs::File;
use std::process::exit;
fn main() -> std::io::Result<()> {
    let path = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: block_print <block_file>");
        exit(1);
    });

    let file = File::open(path)?;
    let block = Block::load(file)?;
    println!("{block:#?}");
    Ok(())
}
