pub mod agents;
pub mod content;
pub mod control;
pub mod hooks;
pub mod mcp_config;
pub mod messages;
pub mod options;
pub mod permissions;
pub mod sandbox;

// Re-exports for convenience.
pub use agents::AgentDefinition;
pub use content::ContentBlock;
pub use control::{
    SDKCapabilities, SDKControlCommand, SDKControlRequest, SDKControlResponse, SDKInitMessage,
    SDKInitResponse,
};
pub use hooks::{
    HookCallback, HookDecision, HookDefinition, HookEvent, HookInput, HookMatcher, HookOutput,
};
pub use mcp_config::{McpServerConfig, McpServerEntry, McpServerStatus};
pub use messages::{AssistantMessage, Message, ResultMessage, Usage, UserMessage};
pub use options::ClaudeAgentOptions;
pub use permissions::{CanUseToolCallback, CanUseToolInput, PermissionMode, PermissionResult};
pub use sandbox::{SandboxSettings, SandboxType};
