mod core;
use anyhow::{Result, anyhow};
use btclib::types::Transaction;
use clap::{Parser, Subcommand};
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    sync::Arc,
};
use tokio::time::{self, Duration};

use crate::core::{Config, Core, FeeType, Recipient};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,
    #[arg(short, long, value_name = "ADDRESS")]
    node: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    GenerateConfig {
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
    },
}

async fn update_utxos(core: Arc<Core>) {
    //TODO read utxo refresh interval from config
    let mut interval = time::interval(Duration::from_secs(20));
    loop {
        interval.tick().await;
        if let Err(e) = core.fetch_utxos().await {
            eprintln!("failed to update UTXOs: {e}")
        }
    }
}

async fn handle_transactions(rx: kanal::AsyncReceiver<Transaction>, core: Arc<Core>) {
    while let Ok(tx) = rx.recv().await {
        if let Err(e) = core.send_transaction(tx).await {
            eprintln!("failed to send transaction: {e}")
        }
    }
}

async fn run_cli(core: Arc<Core>) -> Result<()> {
    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let parts: Vec<_> = input.trim().split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        match parts[0] {
            "balance" => {
                println!("Current balance: {} satoshis", core.get_balance());
            }
            "send" => {
                if parts.len() != 3 {
                    println!("Usage: send <recipient contact name> <amount in sats>");
                    continue;
                }
                let recipient = parts[1].trim();
                let amount: u64 = parts[2].trim().parse()?;
                let recipient_pubkey = core
                    .config
                    .contacts
                    .iter()
                    .find(|r| r.name == recipient)
                    .ok_or_else(|| anyhow!("recipient name not found"))?
                    .load()?
                    .key;

                // TODO why refreshing utxos before sending tx?
                if let Err(e) = core.fetch_utxos().await {
                    println!("Failed to fetch utxos: {e}");
                }

                let tx = core.create_transaction(&recipient_pubkey, amount).await?;
                core.tx_sender.send(tx).await?;
                println!("Transaction sent successfully");
                core.fetch_utxos().await?;
            }
            "exit" => break,
            _ => println!("Unknown commad"),
        }
    }
    Ok(())
}

fn generate_dummy_config(path: &PathBuf) -> Result<()> {
    let dummy_config = Config {
        my_keys: vec![],
        contacts: vec![
            Recipient {
                name: "Alice".to_string(),
                key: PathBuf::from("alice.pub.pem"),
            },
            Recipient {
                name: "Bob".to_string(),
                key: PathBuf::from("bob.pub.pem"),
            },
        ],
        default_node: "127.0.0.1:9000".to_string(),
        fees: FeeType::Percent(0.1),
    };
    let config_str = toml::to_string_pretty(&dummy_config)?;
    std::fs::write(path, config_str)?;
    println!("Dummy config generated at: {}", path.display());
    Ok(())
}

fn default_config_path() -> Result<PathBuf> {
    let mut dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
    dir.push("rsbtc"); // -> ~/rsbtc
    fs::create_dir_all(&dir)?; // create ~/rsbtc if missing

    dir.push("wallet_config.toml"); // -> ~/rsbtc/wallet_config.toml
    Ok(dir)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::GenerateConfig { output }) => {
            let path = match output {
                Some(p) => p,
                None => default_config_path()?,
            };
            return generate_dummy_config(&path);
        }
        None => {}
    }

    let config_path = match cli.config {
        Some(p) => p,
        None => default_config_path()?,
    };

    let (tx_sender, tx_receiver) = kanal::bounded(10);
    let mut core = Core::load(config_path.clone(), tx_sender.clone_async()).await?;
    if let Some(node) = cli.node {
        core.config.default_node = node;
    }

    let core = Arc::new(core);
    tokio::spawn(update_utxos(core.clone()));
    tokio::spawn(handle_transactions(tx_receiver.clone_async(), core.clone()));
    run_cli(core).await?;
    Ok(())
}
