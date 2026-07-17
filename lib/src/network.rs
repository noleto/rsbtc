// network.rs
use crate::crypto::PublicKey;
use crate::types::{Block, Transaction, TransactionOutput};
use serde::{Deserialize, Serialize};
use std::io::{Error as IoError, Read, Write};
use std::time::Duration;
use strum::Display;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MESSAGE_SIZE: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize, Display)]
pub enum Message {
    /// Fetch all UTXOs belonging to a public key
    FetchUTXOs(PublicKey),
    /// UTXOs belonging to a public key. Bool determines if is primed by a pending tx
    UTXOs(Vec<(TransactionOutput, bool)>),
    /// Send a transaction to the network, don't expect any reponse when sending this message
    SubmitTransaction(Transaction),
    /// Broadcast a new transaction to other nodes
    NewTransaction(Transaction),
    /// Ask the node to prepare the optimal block template
    /// with the coinbase transaction paying the specified
    /// public key
    FetchTemplate(PublicKey),
    /// The template
    Template(Block),
    /// This is the response for fetch template when the node cannot serve the template
    TemplateNotAvailable(),
    /// Ask the node to validate a block template.
    /// This is to prevent the node from mining an invalid
    /// block (e.g. if one has been found in the meantime,
    /// or if transactions have been removed from the mempool)
    ValidateTemplate(Block),
    /// If template is valid
    TemplateValidity(bool),
    /// Submit a mined block to a node, don't expect any reponse when sending this message
    SubmitTemplate(Block),
    /// Ask a node to report all the other nodes it knows about
    DiscoverNodes,
    /// This is the response to DiscoverNodes
    NodeList(Vec<String>),
    /// Ask a node whats the highest block it knows about
    AskBlockCount,
    /// This is the response to AskBlockCount
    BlockCount(u64),
    /// Ask a node to send a block with the specified height
    FetchBlock(u64),
    /// This is the response for fetch block when the node has no such block
    BlockNotFound(),
    /// Broadcast a new block to other nodes
    NewBlock(Block),
    /// Generic message when a node receive a message but it doesn't support it
    Unsupported(),
}

/// Handles transport logic and connection to a single peer.
///
/// Owns the underlying stream and knows its own address so it can reconnect
/// after a desync
pub struct Connection {
    addr: String,
    stream: TcpStream,
}

#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("io error: {0}")]
    Io(#[from] IoError),
    #[error("failed to serialize message: {0}")]
    Encode(#[from] ciborium::ser::Error<IoError>),
    #[error("failed to deserialize message: {0}")]
    Decode(#[from] ciborium::de::Error<IoError>),
    #[error("timed out after {0:?}")]
    Timeout(Duration),
    #[error("unexpected message from {addr}")]
    UnexpectedMessage { addr: String },
    #[error("message payload of {0} bytes exceeds maximum allowed ({MAX_MESSAGE_SIZE})")]
    MessageTooLarge(usize),
}

impl Message {
    pub fn encode(&self) -> Result<Vec<u8>, ciborium::ser::Error<IoError>> {
        let mut bytes = vec![];
        ciborium::into_writer(self, &mut bytes)?;
        Ok(bytes)
    }

    pub fn decode(data: &[u8]) -> Result<Self, ciborium::de::Error<IoError>> {
        ciborium::from_reader(data)
    }

    pub fn send(&self, stream: &mut impl Write) -> Result<(), NetworkError> {
        let bytes = self.encode()?;
        let len = bytes.len() as u64;
        stream.write_all(&len.to_be_bytes())?;
        stream.write_all(&bytes)?;
        Ok(())
    }

    pub fn receive(stream: &mut impl Read) -> Result<Self, NetworkError> {
        let mut len_bytes = [0u8; 8]; //len is u64
        stream.read_exact(&mut len_bytes)?;
        let len = u64::from_be_bytes(len_bytes) as usize;
        if len > MAX_MESSAGE_SIZE {
            return Err(NetworkError::MessageTooLarge(len));
        }
        let mut data = vec![0u8; len];
        stream.read_exact(&mut data)?;
        Ok(Self::decode(&data)?)
    }

    pub async fn send_async<W>(&self, stream: &mut W) -> Result<(), NetworkError>
    where
        W: AsyncWrite + Unpin,
    {
        let bytes = self.encode()?;
        let len = bytes.len() as u64;
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(&bytes).await?;
        Ok(())
    }

    pub async fn receive_async<R>(stream: &mut R) -> Result<Self, NetworkError>
    where
        R: AsyncRead + Unpin,
    {
        let mut len_bytes = [0u8; 8];
        stream.read_exact(&mut len_bytes).await?;
        let len = u64::from_be_bytes(len_bytes) as usize;
        if len > MAX_MESSAGE_SIZE {
            return Err(NetworkError::MessageTooLarge(len));
        }
        let mut data = vec![0u8; len];
        stream.read_exact(&mut data).await?;
        Ok(Self::decode(&data)?)
    }
}

impl Connection {
    pub async fn connect(addr: impl Into<String>) -> Result<Self, NetworkError> {
        let addr = addr.into();
        let stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(&addr))
            .await
            .map_err(|_| NetworkError::Timeout(CONNECT_TIMEOUT))??;
        Ok(Self { addr, stream })
    }

    pub fn from_stream(addr: impl Into<String>, stream: TcpStream) -> Self {
        Self {
            addr: addr.into(),
            stream,
        }
    }

    pub async fn send(&mut self, message: &Message) -> Result<(), NetworkError> {
        message.send_async(&mut self.stream).await?;
        Ok(())
    }

    pub async fn receive(&mut self) -> Result<Message, NetworkError> {
        Ok(Message::receive_async(&mut self.stream).await?)
    }

    async fn reconnect(&mut self) -> Result<(), NetworkError> {
        self.stream = TcpStream::connect(&self.addr).await?;
        Ok(())
    }

    pub async fn request(
        &mut self,
        message: &Message,
        timeout: Duration,
    ) -> Result<Message, NetworkError> {
        self.send(message).await?;
        match tokio::time::timeout(timeout, self.receive()).await {
            Ok(Ok(msg)) => Ok(msg),
            Ok(Err(e)) => {
                // Decode desync or an I/O error mid-read — the stream is suspect. Heal it,
                // but surface the real cause so the caller knows what happened.
                self.reconnect().await?;
                Err(e)
            }
            Err(_elapsed) => {
                self.reconnect().await?; // heal the desynced stream
                Err(NetworkError::Timeout(timeout))
            }
        }
    }

    pub async fn request_expect<T>(
        &mut self,
        message: &Message,
        timeout: Duration,
        extract: impl FnOnce(Message) -> Option<T>,
    ) -> Result<T, NetworkError> {
        let reply = self.request(message, timeout).await?;
        let addr = self.addr.clone();
        extract(reply).ok_or_else(|| NetworkError::UnexpectedMessage { addr })
    }
}
