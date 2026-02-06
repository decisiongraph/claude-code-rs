use std::collections::HashMap;
use std::path::PathBuf;

use super::agents::AgentDefinition;
use super::hooks::HookDefinition;
use super::mcp_config::McpServerConfig;
use super::permissions::{CanUseToolCallback, PermissionMode};
use super::sandbox::SandboxSettings;

/// Configuration options for a Claude Agent SDK query or client.
///
/// All fields are public with sensible defaults. Use `..Default::default()` for
/// fields you don't need to set.
#[derive(Default)]
pub struct ClaudeAgentOptions {
    // --- Core ---
    /// The prompt/message to send. Can be set here or passed to query().
    pub prompt: Option<String>,

    /// Model to use (e.g. "claude-sonnet-4-20250514").
    pub model: Option<String>,

    /// System prompt override.
    pub system_prompt: Option<String>,

    /// Append system prompt (added after default).
    pub append_system_prompt: Option<String>,

    /// Maximum turns (agentic loops) before stopping.
    pub max_turns: Option<u32>,

    /// Maximum tokens in the response.
    pub max_tokens: Option<u32>,

    // --- Session ---
    /// Resume an existing session by ID.
    pub session_id: Option<String>,

    /// Continue the most recent session.
    pub continue_session: bool,

    // --- Working directory ---
    /// Working directory for the CLI process.
    pub cwd: Option<PathBuf>,

    // --- Permission ---
    /// Permission mode for tool usage.
    pub permission_mode: PermissionMode,

    /// Specific tools to allow (when using AllowedTools mode).
    pub allowed_tools: Vec<String>,

    /// Custom permission callback.
    pub can_use_tool: Option<CanUseToolCallback>,

    // --- Hooks ---
    /// Registered hook definitions.
    pub hooks: Vec<HookDefinition>,

    // --- MCP ---
    /// MCP servers to register with the CLI.
    pub mcp_servers: HashMap<String, McpServerConfig>,

    // --- Agents ---
    /// Sub-agent definitions.
    pub agents: Vec<AgentDefinition>,

    // --- Sandbox ---
    /// Sandbox configuration.
    pub sandbox: Option<SandboxSettings>,

    // --- CLI flags ---
    /// Additional environment variables for the CLI process.
    pub env: HashMap<String, String>,

    /// Verbose output from CLI.
    pub verbose: bool,

    /// Path to the claude CLI binary (auto-detected if None).
    pub cli_path: Option<PathBuf>,

    /// Custom CLI arguments (appended after built-in ones).
    pub extra_cli_args: Vec<String>,

    /// Timeout for the initial connection handshake.
    pub connect_timeout: Option<std::time::Duration>,

    /// Timeout for control protocol requests.
    pub control_timeout: Option<std::time::Duration>,

    /// Stderr callback - receives stderr lines from CLI process.
    pub on_stderr: Option<StderrCallback>,

    /// Disallow use of the prompt cache
    pub no_cache: bool,

    /// Temperature setting
    pub temperature: Option<f64>,

    /// Context window fraction (0.0-1.0) to use before summarizing.
    pub context_window: Option<f64>,
}

/// Callback for CLI stderr lines.
pub type StderrCallback =
    std::sync::Arc<dyn Fn(String) + Send + Sync>;

impl std::fmt::Debug for ClaudeAgentOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeAgentOptions")
            .field("model", &self.model)
            .field("prompt", &self.prompt.as_ref().map(|p| {
                if p.len() > 50 { format!("{}...", &p[..50]) } else { p.clone() }
            }))
            .field("max_turns", &self.max_turns)
            .field("session_id", &self.session_id)
            .field("permission_mode", &self.permission_mode)
            .field("verbose", &self.verbose)
            .field("hooks_count", &self.hooks.len())
            .field("mcp_servers_count", &self.mcp_servers.len())
            .field("agents_count", &self.agents.len())
            .finish_non_exhaustive()
    }
}
