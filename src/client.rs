use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::error::{Error, Result};
use crate::mcp::SdkMcpServer;
use crate::query::{McpMessageHandler, Query};
use crate::transport::cli_discovery;
use crate::transport::subprocess::SubprocessTransport;
use crate::types::messages::Message;
use crate::types::options::ClaudeAgentOptions;

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
/// let mut stream = client.receive_messages();
/// while let Some(msg) = stream.next().await {
///     let msg = msg?;
///     if msg.is_result() { break; }
/// }
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
    mcp_servers: Vec<SdkMcpServer>,
}

impl ClaudeSDKClient {
    pub fn new(options: ClaudeAgentOptions) -> Self {
        Self {
            options,
            query: None,
            message_rx: None,
            mcp_servers: Vec::new(),
        }
    }

    /// Register an in-process MCP server.
    pub fn add_mcp_server(&mut self, server: SdkMcpServer) {
        self.mcp_servers.push(server);
    }

    /// Connect to the Claude CLI. Optionally send an initial prompt.
    pub async fn connect(&mut self, initial_prompt: Option<&str>) -> Result<()> {
        if self.query.is_some() {
            return Err(Error::AlreadyConnected);
        }

        let cli_path = match self.options.cli_path {
            Some(ref p) => p.clone(),
            None => cli_discovery::find_cli()?,
        };

        let transport = SubprocessTransport::new(cli_path, &self.options);

        let mcp_handler = self.build_mcp_handler();

        let mut q = Query::new(
            Box::new(transport),
            std::mem::take(&mut self.options.hooks),
            self.options.can_use_tool.take(),
            mcp_handler,
            self.options.control_timeout,
        );

        let rx = q.connect().await?;
        self.message_rx = Some(rx);
        self.query = Some(q);

        if let Some(prompt) = initial_prompt {
            self.query
                .as_ref()
                .unwrap()
                .send_message(prompt, None)
                .await?;
        }

        Ok(())
    }

    /// Send a query/prompt. Optionally provide a session_id for resuming.
    pub async fn query(&self, prompt: &str, session_id: Option<&str>) -> Result<()> {
        let q = self.query.as_ref().ok_or(Error::NotConnected)?;
        q.send_message(prompt, session_id).await
    }

    /// Get a stream of messages from the current query.
    ///
    /// Messages flow until a `ResultMessage` signals end of turn.
    pub fn receive_messages(&mut self) -> ReceiverStream<Result<Message>> {
        let rx = self.message_rx.take().unwrap_or_else(|| {
            let (_tx, rx) = mpsc::channel(1);
            rx
        });
        ReceiverStream::new(rx)
    }

    /// Collect all messages until the next ResultMessage.
    pub async fn receive_response(&mut self) -> Result<Vec<Message>> {
        use tokio_stream::StreamExt;

        let mut stream = self.receive_messages();
        let mut messages = Vec::new();

        while let Some(msg) = stream.next().await {
            let msg = msg?;
            let is_result = msg.is_result();
            messages.push(msg);
            if is_result {
                break;
            }
        }

        // Put the receiver back (the stream may have more messages).
        self.message_rx = Some(stream.into_inner());

        Ok(messages)
    }

    /// Send an interrupt command.
    pub async fn interrupt(&self) -> Result<Value> {
        let q = self.query.as_ref().ok_or(Error::NotConnected)?;
        q.interrupt().await
    }

    /// Change the permission mode.
    pub async fn set_permission_mode(&self, mode: &str) -> Result<Value> {
        let q = self.query.as_ref().ok_or(Error::NotConnected)?;
        q.set_permission_mode(mode).await
    }

    /// Change the model.
    pub async fn set_model(&self, model: &str) -> Result<Value> {
        let q = self.query.as_ref().ok_or(Error::NotConnected)?;
        q.set_model(model).await
    }

    /// Rewind file changes to a specific user message.
    pub async fn rewind_files(&self, user_message_id: &str) -> Result<Value> {
        let q = self.query.as_ref().ok_or(Error::NotConnected)?;
        q.rewind_files(user_message_id).await
    }

    /// Get MCP server status.
    pub async fn get_mcp_status(&self) -> Result<Value> {
        let q = self.query.as_ref().ok_or(Error::NotConnected)?;
        q.get_mcp_status().await
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

        // For simplicity, we handle only the first MCP server.
        // A full implementation would look up by server_name.
        // TODO: support multiple named MCP servers.
        None
        // MCP handler will be wired when we have named server lookup.
        // The control protocol provides the server_name in the request.
    }
}
