use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Permission mode for tool usage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    /// Default permissions - prompt user for dangerous tools.
    Default,
    /// Accept all tool uses without prompting.
    AcceptAll,
    /// Deny all tool uses.
    DenyAll,
    /// Use allowed-tools list from CLI config.
    AllowedTools,
}

impl Default for PermissionMode {
    fn default() -> Self {
        Self::Default
    }
}

/// Result from a permission check callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResult {
    /// Whether the tool use is allowed.
    pub allowed: bool,
    /// Optional reason for denial.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl PermissionResult {
    pub fn allow() -> Self {
        Self {
            allowed: true,
            reason: None,
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.into()),
        }
    }
}

/// Input provided to the can_use_tool callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanUseToolInput {
    pub tool_name: String,
    pub input: Value,
}

/// Async callback for permission checks.
pub type CanUseToolCallback = Arc<
    dyn Fn(CanUseToolInput) -> Pin<Box<dyn Future<Output = PermissionResult> + Send>>
        + Send
        + Sync,
>;

/// Helper to create a CanUseToolCallback from a closure.
pub fn permission_callback<F, Fut>(f: F) -> CanUseToolCallback
where
    F: Fn(CanUseToolInput) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = PermissionResult> + Send + 'static,
{
    Arc::new(move |input| Box::pin(f(input)))
}
