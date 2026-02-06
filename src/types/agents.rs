use serde::{Deserialize, Serialize};

/// Definition of a sub-agent that Claude can delegate to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Unique name for this agent.
    pub name: String,

    /// Description of what this agent does.
    pub description: String,

    /// System prompt for the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,

    /// Tools allowed for this agent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,

    /// Model to use for this agent (overrides default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}
