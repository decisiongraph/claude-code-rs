use std::io;

/// All errors that can occur in the Claude Agent SDK.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("claude CLI not found in PATH")]
    CliNotFound,

    #[error("claude CLI version {found} too old, need >= {required}")]
    CliVersionTooOld { found: String, required: String },

    #[error("failed to connect to CLI process: {0}")]
    CliConnection(String),

    #[error("CLI process error: {0}")]
    Process(String),

    #[error("CLI process exited with code {code}: {stderr}")]
    ProcessExit { code: i32, stderr: String },

    #[error("JSON decode error: {0}")]
    JsonDecode(#[from] serde_json::Error),

    #[error("failed to parse message: {reason}")]
    MessageParse { reason: String },

    #[error("control protocol timeout after {0:?}")]
    ControlTimeout(std::time::Duration),

    #[error("control protocol error: {0}")]
    ControlProtocol(String),

    #[error("transport closed")]
    TransportClosed,

    #[error("not connected")]
    NotConnected,

    #[error("already connected")]
    AlreadyConnected,

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("MCP error: {code}: {message}")]
    Mcp { code: i64, message: String },

    #[error("hook error: {0}")]
    Hook(String),
}

pub type Result<T> = std::result::Result<T, Error>;
