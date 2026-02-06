use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use serde_json::Value;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;

use crate::error::{Error, Result};
use crate::mcp::SdkMcpServer;
use crate::query::{McpMessageHandler, Query};
use crate::transport::subprocess::SubprocessTransport;
use crate::types::messages::Message;
use crate::types::options::ClaudeAgentOptions;

/// RAII guard that returns the receiver back to the client on drop.
///
/// Implements [`Stream`] by delegating to the inner [`ReceiverStream`],
/// so `stream.next().await` works as expected.
pub struct MessageStream<'a> {
    inner: Option<ReceiverStream<Result<Message>>>,
    slot: &'a mut Option<mpsc::Receiver<Result<Message>>>,
}

impl<'a> MessageStream<'a> {
    fn new(
        stream: ReceiverStream<Result<Message>>,
        slot: &'a mut Option<mpsc::Receiver<Result<Message>>>,
    ) -> Self {
        Self {
            inner: Some(stream),
            slot,
        }
    }
}

impl Stream for MessageStream<'_> {
    type Item = Result<Message>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.as_mut() {
            Some(stream) => Pin::new(stream).poll_next(cx),
            None => Poll::Ready(None),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.inner {
            Some(stream) => stream.size_hint(),
            None => (0, Some(0)),
        }
    }
}

impl Drop for MessageStream<'_> {
    fn drop(&mut self) {
        if let Some(stream) = self.inner.take() {
            *self.slot = Some(stream.into_inner());
        }
    }
}

/// A stateful client for multi-turn conversations with the Claude CLI.
///
/// Unlike `query()` which is one-shot, the client maintains a connection
/// and supports sending multiple queries, interrupts, and control commands.
///
/// # Example
/// ```no_run
/// use claude_code_rs::{ClaudeAgentOptions, Message};
/// use claude_code_rs::client::ClaudeSDKClient;
/// use tokio_stream::StreamExt;
///
/// # async fn example() -> claude_code_rs::Result<()> {
/// let mut client = ClaudeSDKClient::new(ClaudeAgentOptions::default());
/// client.connect(None).await?;
///
/// // First query.
/// client.query("What is Rust?", None).await?;
/// {
///     let mut stream = client.receive_messages();
///     while let Some(msg) = stream.next().await {
///         let msg = msg?;
///         if msg.is_result() { break; }
///     }
/// } // Receiver auto-restores when `stream` drops.
///
/// // Follow-up query in same session.
/// client.query("How does ownership work?", None).await?;
/// // ...
///
/// client.disconnect().await?;
/// # Ok(())
/// # }
/// ```
pub struct ClaudeSDKClient {
    options: ClaudeAgentOptions,
    query: Option<Query>,
    message_rx: Option<mpsc::Receiver<Result<Message>>>,
    mcp_servers: HashMap<String, Arc<Mutex<SdkMcpServer>>>,
}

impl ClaudeSDKClient {
    fn query_ref(&self) -> Result<&Query> {
        self.query.as_ref().ok_or(Error::NotConnected)
    }

    #[must_use]
    pub fn new(options: ClaudeAgentOptions) -> Self {
        Self {
            options,
            query: None,
            message_rx: None,
            mcp_servers: HashMap::new(),
        }
    }

    /// Register an in-process MCP server by name.
    ///
    /// Must be called **before** [`connect()`](Self::connect). Returns an error
    /// if the client is already connected (servers are snapshot-cloned during connect).
    pub fn add_mcp_server(
        &mut self,
        name: impl Into<String>,
        server: SdkMcpServer,
    ) -> Result<()> {
        if self.is_connected() {
            return Err(Error::AlreadyConnected);
        }
        self.mcp_servers
            .insert(name.into(), Arc::new(Mutex::new(server)));
        Ok(())
    }

    /// Connect to the Claude CLI. Optionally send an initial prompt.
    pub async fn connect(&mut self, initial_prompt: Option<&str>) -> Result<()> {
        if self.query.is_some() {
            return Err(Error::AlreadyConnected);
        }

        let cli_path = self.options.resolve_cli_path()?;
        let transport = SubprocessTransport::new(cli_path, &self.options);

        let mcp_handler = self.build_mcp_handler();

        let mut q = Query::new(
            Box::new(transport),
            self.options.hooks.clone(),
            self.options.can_use_tool.clone(),
            mcp_handler,
            self.options.control_timeout,
        );

        let rx = q.connect().await?;
        self.message_rx = Some(rx);
        self.query = Some(q);

        if let Some(prompt) = initial_prompt {
            self.query_ref()?.send_message(prompt, None).await?;
        }

        Ok(())
    }

    /// Send a query/prompt. Optionally provide a session_id for resuming.
    pub async fn query(&self, prompt: &str, session_id: Option<&str>) -> Result<()> {
        self.query_ref()?.send_message(prompt, session_id).await
    }

    /// Get a stream of messages from the current query.
    ///
    /// Messages flow until a `ResultMessage` signals end of turn.
    /// The receiver is automatically restored when the returned
    /// [`MessageStream`] is dropped, so the client remains usable
    /// for follow-up queries.
    pub fn receive_messages(&mut self) -> MessageStream<'_> {
        let rx = self.message_rx.take().unwrap_or_else(|| {
            let (_tx, rx) = mpsc::channel(1);
            rx
        });
        MessageStream::new(ReceiverStream::new(rx), &mut self.message_rx)
    }

    /// Collect all messages until the next ResultMessage.
    pub async fn receive_response(&mut self) -> Result<Vec<Message>> {
        use crate::types::messages::collect_until_result;

        let mut stream = self.receive_messages();
        collect_until_result(&mut stream).await
        // Receiver auto-restores when `stream` drops.
    }

    /// Send an interrupt command.
    pub async fn interrupt(&self) -> Result<Value> {
        self.query_ref()?.interrupt().await
    }

    /// Change the permission mode.
    pub async fn set_permission_mode(&self, mode: &str) -> Result<Value> {
        self.query_ref()?.set_permission_mode(mode).await
    }

    /// Change the model.
    pub async fn set_model(&self, model: &str) -> Result<Value> {
        self.query_ref()?.set_model(model).await
    }

    /// Rewind file changes to a specific user message.
    pub async fn rewind_files(&self, user_message_id: &str) -> Result<Value> {
        self.query_ref()?.rewind_files(user_message_id).await
    }

    /// Get MCP server status.
    pub async fn get_mcp_status(&self) -> Result<Value> {
        self.query_ref()?.get_mcp_status().await
    }

    /// Get server info from the init handshake.
    pub async fn get_server_info(&self) -> Option<Value> {
        match &self.query {
            Some(q) => q.get_server_info().await,
            None => None,
        }
    }

    /// Disconnect from the CLI.
    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(mut q) = self.query.take() {
            q.close().await?;
        }
        self.message_rx = None;
        Ok(())
    }

    /// Check if connected.
    pub fn is_connected(&self) -> bool {
        self.query.is_some()
    }

    fn build_mcp_handler(&self) -> Option<McpMessageHandler> {
        if self.mcp_servers.is_empty() {
            return None;
        }

        let servers = self.mcp_servers.clone();
        Some(Arc::new(move |server_name: String, message: Value| {
            let servers = servers.clone();
            Box::pin(async move {
                if let Some(server) = servers.get(&server_name) {
                    let srv = server.lock().await;
                    srv.handle_message(message).await
                } else {
                    serde_json::json!({"error": format!("unknown MCP server: {server_name}")})
                }
            })
        }))
    }
}
