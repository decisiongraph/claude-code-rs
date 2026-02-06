use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A control command sent from the SDK to the CLI.
#[derive(Debug, Clone)]
pub enum SDKControlCommand {
    Interrupt,
    SetPermissionMode { mode: String },
    SetModel { model: String },
    RewindFiles { user_message_id: String },
    GetMcpStatus,
}

impl SDKControlCommand {
    pub fn interrupt() -> Self {
        Self::Interrupt
    }

    pub fn set_permission_mode(mode: &str) -> Self {
        Self::SetPermissionMode { mode: mode.into() }
    }

    pub fn set_model(model: &str) -> Self {
        Self::SetModel { model: model.into() }
    }

    pub fn rewind_files(user_message_id: &str) -> Self {
        Self::RewindFiles { user_message_id: user_message_id.into() }
    }

    pub fn get_mcp_status() -> Self {
        Self::GetMcpStatus
    }

    /// Build the full request body for the control protocol.
    pub fn to_request_body(&self) -> Value {
        match self {
            Self::Interrupt => serde_json::json!({"subtype": "interrupt"}),
            Self::SetPermissionMode { mode } => serde_json::json!({"subtype": "set_permission_mode", "mode": mode}),
            Self::SetModel { model } => serde_json::json!({"subtype": "set_model", "model": model}),
            Self::RewindFiles { user_message_id } => serde_json::json!({"subtype": "rewind_files", "user_message_id": user_message_id}),
            Self::GetMcpStatus => serde_json::json!({"subtype": "get_mcp_status"}),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SDKCapabilities {
    #[serde(default)]
    pub hooks: bool,
    #[serde(default)]
    pub permissions: bool,
    #[serde(default)]
    pub mcp: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_definitions: Vec<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<Value>,
}

