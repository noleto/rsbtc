use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::Duration;

use anyhow::{Result, anyhow};
use btclib::crypto::PublicKey;
use btclib::network::{Connection, DEFAULT_REQUEST_TIMEOUT, Message};
use btclib::types::Block;
use btclib::utils::Saveable;
use clap::Parser;
use std::sync::atomic::Ordering;
use tokio::sync::Mutex;
use tokio::time::interval;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long)]
    address: String,
    #[arg(short, long)]
    public_key_file: String,
}

struct Miner {
    public_key: PublicKey,
    conn: Mutex<Connection>,
    current_template: Arc<std::sync::Mutex<Option<Block>>>,
    mining: Arc<AtomicBool>,
    mined_block_sender: flume::Sender<Block>,
    mined_block_receiver: flume::Receiver<Block>,
}

impl Miner {
    async fn new(address: String, public_key: PublicKey) -> Result<Self> {
        let (mined_block_sender, mined_block_receiver) = flume::unbounded();

        Ok(Self {
            public_key,
            conn: Mutex::new(Connection::connect(address).await?),
            current_template: Arc::new(std::sync::Mutex::new(None)),
            mining: Arc::new(AtomicBool::new(false)),
            mined_block_sender,
            mined_block_receiver,
        })
    }
    async fn run(&self) -> Result<()> {
        self.spawn_mining_thread();
        let mut template_interval = interval(Duration::from_secs(5));
        let receiver = self.mined_block_receiver.clone();
        loop {
            tokio::select! {
                _ = template_interval.tick() => {
                    if let Err(e) = self.fetch_and_validate_template().await {
                            eprintln!("template fetch failed: {e}");   // log and keep looping
                    }
                }
                Ok(mined_block) = receiver.recv_async() => {
                    if let Err(e) = self.submit_block(mined_block).await {
                        eprintln!("block submit failed: {e}")
                    }

                }
            }
        }
    }
    fn spawn_mining_thread(&self) -> thread::JoinHandle<()> {
        let template = self.current_template.clone();
        let mining = self.mining.clone();
        let sender = self.mined_block_sender.clone();

        thread::spawn(move || {
            loop {
                if mining.load(Ordering::Relaxed) {
                    if let Some(mut block) = template.lock().unwrap().clone() {
                        println!("Mining block with target: {}", block.header.target);
                        if block.header.mine(2_000_000) {
                            println!("Block mined: {}", block.hash());
                            sender.send(block).expect("should send the mined block");
                            mining.store(false, Ordering::Relaxed);
                        }
                    }
                }
                thread::yield_now();
            }
        })
    }
    async fn fetch_and_validate_template(&self) -> Result<()> {
        if !self.mining.load(Ordering::Relaxed) {
            self.fetch_template().await?;
        } else {
            self.validate_template().await?;
        }
        Ok(())
    }

    async fn fetch_template(&self) -> Result<()> {
        println!("Fetching new template");
        let message = Message::FetchTemplate(self.public_key.clone());

        let template = self
            .conn
            .lock()
            .await
            .request_expect(&message, DEFAULT_REQUEST_TIMEOUT, |m| match m {
                Message::Template(t) => Some(t),
                _ => None,
            })
            .await?;

        println!(
            "Received new template with target: {}",
            template.header.target
        );
        let mut template_guard = self.current_template.lock().unwrap();
        *template_guard = Some(template);
        self.mining.store(true, Ordering::Relaxed);
        Ok(())
    }
    async fn validate_template(&self) -> Result<()> {
        let maybe_template = self.current_template.lock().unwrap().clone(); // guard dropped here
        match maybe_template {
            Some(template) => {
                let message = Message::ValidateTemplate(template);
                let valid = self
                    .conn
                    .lock()
                    .await
                    .request_expect(&message, DEFAULT_REQUEST_TIMEOUT, |m| match m {
                        Message::TemplateValidity(b) => Some(b),
                        _ => None,
                    })
                    .await?;

                if !valid {
                    println!("Current template is no longer valid");
                    self.mining.store(false, Ordering::Relaxed);
                } else {
                    println!("Current template is still valid");
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    async fn submit_block(&self, block: Block) -> Result<()> {
        println!("Submitting mined block");
        let message = Message::SubmitTemplate(block);
        self.conn.lock().await.send(&message).await?;
        self.mining.store(false, Ordering::Relaxed);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let public_key = PublicKey::load_from_file(&cli.public_key_file)
        .map_err(|e| anyhow!("Error reading public key: {}", e))?;

    let miner = Miner::new(cli.address, public_key).await?;
    miner.run().await
}
