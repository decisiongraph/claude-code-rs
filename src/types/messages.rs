use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_stream::{Stream, StreamExt};

use super::content::ContentBlock;
use crate::error::Result;

/// A message from the Claude CLI streaming protocol.
///
/// The CLI emits newline-delimited JSON objects with a top-level `type` field.
/// Each variant corresponds to one of these message types.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Message {
    /// System-level message (init acknowledgment, etc.)
    System {
        subtype: String,
        data: Value,
    },

    /// Assistant (Claude) response message.
    Assistant {
        message: AssistantMessage,
    },

    /// User message echo.
    User {
        message: UserMessage,
    },

    /// Result/completion message - signals end of a turn.
    Result {
        result: ResultMessage,
    },

    /// An unknown message type we don't recognize but preserve.
    Unknown {
        message_type: String,
        raw: Value,
    },
}

/// An assistant response with content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub content: Vec<ContentBlock>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
    /// Raw extra fields we don't explicitly model.
    #[serde(flatten)]
    pub extra: Value,
}

/// A user message as echoed back by the CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub content: UserContent,
    #[serde(flatten)]
    pub extra: Value,
}

/// User message content can be a string or structured blocks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum UserContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
    #[default]
    Empty,
}

/// Result message indicating the end of a query turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultMessage {
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<f64>,
    #[serde(default)]
    pub duration_api_ms: Option<f64>,
    #[serde(default)]
    pub num_turns: Option<u32>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(flatten)]
    pub extra: Value,
}

/// Token usage information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(flatten)]
    pub extra: Value,
}

impl Message {
    /// Returns true if this is a Result message (end of turn).
    pub fn is_result(&self) -> bool {
        matches!(self, Message::Result { .. })
    }

    /// Returns true if this is a Result message with an error.
    pub fn is_error(&self) -> bool {
        matches!(self, Message::Result { result } if result.is_error)
    }

    /// Extract all text content from an Assistant message.
    pub fn text(&self) -> Option<String> {
        match self {
            Message::Assistant { message } => {
                let mut result = String::new();
                for block in &message.content {
                    if let Some(text) = block.as_text() {
                        result.push_str(text);
                    }
                }
                if result.is_empty() { None } else { Some(result) }
            }
            _ => None,
        }
    }

    /// Get the session ID from a Result message.
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Message::Result { result } => result.session_id.as_deref(),
            _ => None,
        }
    }
}

/// Collect messages from a stream until a Result message is received.
pub(crate) async fn collect_until_result(stream: &mut (impl Stream<Item = Result<Message>> + Unpin)) -> Result<Vec<Message>> {
    let mut messages = Vec::new();
    while let Some(msg) = stream.next().await {
        let msg = msg?;
        let is_result = msg.is_result();
        messages.push(msg);
        if is_result {
            break;
        }
    }
    Ok(messages)
}
