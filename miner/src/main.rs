use std::env;
use std::process::exit;

use btclib::types::Block;
use btclib::utils::Saveable;
fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().skip(1).take(2).collect();
    let [path, steps] = args.as_slice() else {
        eprintln!("Usage: miner <block_file> <steps>");
        exit(1);
    };

    // parse steps count
    let steps = steps.parse::<usize>().map_or_else(
        |_| {
            eprintln!("<steps> should be positive number");
            exit(1);
        },
        |ok| {
            if ok == 0 {
                eprintln!("<steps> should greather than 0");
                exit(1);
            }
            ok
        },
    );

    let og_block = Block::load_from_file(path)?;
    let mut block = og_block.clone();

    while !block.header.mine(steps) {
        println!("mining...");
    }

    // print original block and its hash
    println!("original: {:#?}", og_block);
    println!("original block header hash: {}", og_block.header.hash());
    // print mined block and its hash
    println!("final: {:#?}", block);
    println!("final block header hash: {}", block.header.hash());

    Ok(())
}
