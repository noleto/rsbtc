use std::{fs, path::PathBuf, sync::Arc};

use anyhow::{Ok, Result, anyhow};
use btclib::{
    crypto::{PrivateKey, PublicKey, Signature},
    network::{Connection, DEFAULT_REQUEST_TIMEOUT, Message},
    types::{Transaction, TransactionInput, TransactionOutput},
    utils::Saveable,
};
use crossbeam_skiplist::SkipMap;
use kanal::AsyncSender;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Serialize, Deserialize, Clone)]
pub struct Key {
    public: PathBuf,
    private: PathBuf,
}

#[derive(Clone)]
pub struct LoadedKey {
    public: PublicKey,
    private: PrivateKey,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Recipient {
    pub name: String,
    pub key: PathBuf,
}

#[derive(Clone)]
pub struct LoadedRecipient {
    pub key: PublicKey,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum FeeType {
    Fixed(u64),
    Percent(f64),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub my_keys: Vec<Key>,
    pub contacts: Vec<Recipient>,
    pub default_node: String,
    pub fees: FeeType,
}

#[derive(Clone)]
struct Store {
    my_keys: Vec<LoadedKey>,
    utxos_per_key: Arc<SkipMap<PublicKey, Vec<(bool, TransactionOutput)>>>,
}

#[derive(Clone)]
pub struct Core {
    pub config: Config,
    store: Store,
    pub tx_sender: AsyncSender<Transaction>,
    conn: Arc<Mutex<Connection>>,
}

impl Recipient {
    pub fn load(&self) -> Result<LoadedRecipient> {
        let key = PublicKey::load_from_file(&self.key)?;
        Ok(LoadedRecipient { key })
    }
}

impl Store {
    fn new() -> Self {
        Store {
            my_keys: Vec::new(),
            utxos_per_key: Arc::new(SkipMap::new()),
        }
    }
    fn add_key(&mut self, key: LoadedKey) {
        self.my_keys.push(key);
    }

    fn find_private_key(&self, pubkey: &PublicKey) -> Result<&PrivateKey> {
        let loaded_key = &self
            .my_keys
            .iter()
            .find(|k| k.public == *pubkey)
            .ok_or_else(|| {
                anyhow!("no private key not found in your store for pubkey: {pubkey}!")
            })?;
        Ok(&loaded_key.private)
    }
}

impl Core {
    fn new(
        config: Config,
        store: Store,
        tx_sender: AsyncSender<Transaction>,
        conn: Connection,
    ) -> Self {
        Core {
            config,
            store,
            tx_sender,
            conn: Arc::new(Mutex::new(conn)),
        }
    }
    pub async fn load(config_path: PathBuf, sender: AsyncSender<Transaction>) -> Result<Self> {
        let config: Config = toml::from_str(&fs::read_to_string(&config_path)?)?;
        let mut store = Store::new();

        for key in &config.my_keys {
            let pubkey = PublicKey::load_from_file(&key.public)
                .map_err(|e| anyhow!("unable to load public key {}: {e}", &key.public.display()))?;
            let privkey = PrivateKey::load_from_file(&key.private)?;
            store.add_key(LoadedKey {
                public: pubkey,
                private: privkey,
            });
        }

        let conn = Connection::connect(&config.default_node).await?;

        Ok(Core::new(config, store, sender, conn))
    }
    pub async fn fetch_utxos(&self) -> Result<()> {
        let mut conn = self.conn.lock().await;

        for key in &self.store.my_keys {
            let message = Message::FetchUTXOs(key.public.clone());
            let utxos = conn
                .request_expect(&message, DEFAULT_REQUEST_TIMEOUT, |m| match m {
                    Message::UTXOs(utxos) => Some(utxos),
                    _ => None,
                })
                .await?;

            self.store.utxos_per_key.insert(
                key.public.clone(),
                utxos
                    .into_iter()
                    .map(|(output, marked)| (marked, output))
                    .collect(),
            );
        }
        Ok(())
    }

    pub async fn send_transaction(&self, transaction: Transaction) -> Result<()> {
        let mut conn = self.conn.lock().await;
        let message = Message::SubmitTransaction(transaction);
        conn.send(&message).await?;
        Ok(())
    }

    pub fn get_balance(&self) -> u64 {
        self.store
            .utxos_per_key
            .iter()
            .map(|e| e.value().iter().map(|(_, utxo)| utxo.value).sum::<u64>())
            .sum()
    }
    pub async fn create_transaction(
        &self,
        recipient: &PublicKey,
        amount: u64,
    ) -> Result<Transaction> {
        let fees = self.calculate_fee(amount);
        let total_amout = amount + fees;

        let mut inputs = Vec::new();
        let mut input_sum = 0;

        for entry in self.store.utxos_per_key.iter() {
            let pubkey = entry.key();
            let utxos = entry.value();

            for (marked, utxo) in utxos.iter() {
                if *marked {
                    continue; // Skip primed UTXOs (already marked by another spending)
                }
                if input_sum >= total_amout {
                    break;
                }
                let tx_input = TransactionInput {
                    prev_transaction_output_hash: utxo.hash(),
                    signature: Signature::sign_output(
                        &utxo.hash(),
                        self.store.find_private_key(pubkey)?,
                    ),
                };
                inputs.push(tx_input);
                input_sum += utxo.value;
            }
            if input_sum >= total_amout {
                break;
            }
        }

        if input_sum < total_amout {
            return Err(anyhow!("Insufficient funds"));
        }

        let mut outputs = vec![TransactionOutput {
            value: amount,
            unique_id: uuid::Uuid::new_v4(),
            pubkey: recipient.clone(),
        }];

        // create change
        if input_sum > total_amout {
            let change = TransactionOutput {
                value: input_sum - total_amout,
                unique_id: uuid::Uuid::new_v4(),
                pubkey: self.store.my_keys[0].public.clone(),
            };
            outputs.push(change);
        }

        Ok(Transaction::new(inputs, outputs))
    }

    fn calculate_fee(&self, amount: u64) -> u64 {
        match self.config.fees {
            FeeType::Fixed(sats) => sats,
            FeeType::Percent(perc) => (amount as f64 * perc / 100.0) as u64,
        }
    }
}
