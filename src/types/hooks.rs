use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Hook events that can be intercepted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HookEvent {
    #[serde(rename = "preToolUse")]
    PreToolUse,
    #[serde(rename = "postToolUse")]
    PostToolUse,
    #[serde(rename = "notification")]
    Notification,
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "subagentStop")]
    SubagentStop,
}

/// Matcher for which tool/event a hook applies to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookMatcher {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

/// Input for a preToolUse hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreToolUseInput {
    pub tool_name: String,
    pub tool_input: Value,
}

/// Input for a postToolUse hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolUseInput {
    pub tool_name: String,
    pub tool_input: Value,
    pub tool_output: Value,
}

/// Input for a notification hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationInput {
    pub title: String,
    #[serde(default)]
    pub message: Option<String>,
}

/// Input for a stop hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopInput {
    #[serde(default)]
    pub reason: Option<String>,
}

/// Discriminated hook input passed to callbacks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HookInput {
    PreToolUse(PreToolUseInput),
    PostToolUse(PostToolUseInput),
    Notification(NotificationInput),
    Stop(StopInput),
}

/// Output from a hook callback.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookOutput {
    /// If set, blocks the tool use with this reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<HookDecision>,
    /// Optional reason/message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HookDecision {
    Approve,
    Block,
    Ignore,
}

impl HookOutput {
    pub fn approve() -> Self {
        Self {
            decision: Some(HookDecision::Approve),
            reason: None,
        }
    }

    pub fn block(reason: impl Into<String>) -> Self {
        Self {
            decision: Some(HookDecision::Block),
            reason: Some(reason.into()),
        }
    }

    pub fn ignore() -> Self {
        Self {
            decision: Some(HookDecision::Ignore),
            reason: None,
        }
    }
}

/// A registered hook definition.
#[derive(Clone)]
pub struct HookDefinition {
    pub event: HookEvent,
    pub matcher: HookMatcher,
    pub callback: HookCallback,
}

impl std::fmt::Debug for HookDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookDefinition")
            .field("event", &self.event)
            .field("matcher", &self.matcher)
            .field("callback", &"<fn>")
            .finish()
    }
}

/// Async hook callback type.
pub type HookCallback =
    Arc<dyn Fn(HookInput) -> Pin<Box<dyn Future<Output = HookOutput> + Send>> + Send + Sync>;

/// Helper to create a HookCallback from an async closure.
pub fn hook_callback<F, Fut>(f: F) -> HookCallback
where
    F: Fn(HookInput) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HookOutput> + Send + 'static,
{
    Arc::new(move |input| Box::pin(f(input)))
}
