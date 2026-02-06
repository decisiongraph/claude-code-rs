pub mod client;
pub mod error;
pub mod mcp;
pub(crate) mod message_parser;
pub(crate) mod query;
pub mod query_fn;
pub(crate) mod transport;
pub mod types;

// Re-export key types at crate root for ergonomic use.
pub use error::{Error, Result};
pub use types::{
    AssistantMessage, ClaudeAgentOptions, ContentBlock, Message, PermissionMode, PermissionResult,
    ResultMessage, Usage, UserMessage,
};

// Re-export primary APIs.
pub use client::{ClaudeSDKClient, MessageStream};
pub use query_fn::{query, query_collect, query_text};

// Re-export hook helpers.
pub use types::hooks::{hook_callback, HookDefinition, HookEvent, HookMatcher, HookOutput};

// Re-export permission helpers.
pub use types::permissions::permission_callback;

// Re-export MCP helpers.
pub use mcp::{new_tool, McpTool, McpToolResult, SdkMcpServer};
