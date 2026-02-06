pub mod cli_discovery;
pub mod subprocess;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::error::Result;

/// A clonable handle for writing JSON messages to the transport.
///
/// This can be shared across tasks (router, user code, etc.) to write
/// messages back to the CLI process stdin.
#[derive(Clone)]
pub struct TransportWriter {
    tx: mpsc::Sender<Value>,
}

impl TransportWriter {
    pub fn new(tx: mpsc::Sender<Value>) -> Self {
        Self { tx }
    }

    /// Write a JSON message. Returns error if the transport is closed.
    pub async fn write(&self, message: Value) -> Result<()> {
        self.tx
            .send(message)
            .await
            .map_err(|_| crate::error::Error::TransportClosed)
    }
}

/// Trait for a transport layer that communicates with the Claude CLI.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Connect to the CLI process.
    ///
    /// Returns a receiver for incoming messages and a writer for outgoing messages.
    async fn connect(&mut self) -> Result<(mpsc::Receiver<Result<Value>>, TransportWriter)>;

    /// Signal end of input (close stdin).
    async fn end_input(&self) -> Result<()>;

    /// Close the transport and kill the process.
    async fn close(&mut self) -> Result<()>;

    /// Check if the transport is still connected.
    fn is_ready(&self) -> bool;
}
