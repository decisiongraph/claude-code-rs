use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::hooks::HookOutput;
use super::permissions::PermissionResult;

/// A control protocol request from the CLI to the SDK.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKControlRequest {
    /// Unique request ID for correlation.
    pub id: String,
    /// The type of control request.
    #[serde(rename = "type")]
    pub request_type: ControlRequestType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ControlRequestType {
    /// Permission check for tool usage.
    CanUseTool {
        tool_name: String,
        input: Value,
    },
    /// Hook callback invocation.
    HookCallback {
        hook_event: String,
        hook_input: Value,
    },
    /// MCP JSONRPC message routed to an SDK MCP server.
    McpMessage {
        server_name: String,
        message: Value,
    },
}

/// A control protocol response from the SDK to the CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKControlResponse {
    /// Correlation ID matching the request.
    pub id: String,
    /// Response payload.
    #[serde(flatten)]
    pub body: ControlResponseBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ControlResponseBody {
    Permission(PermissionResult),
    Hook(HookOutput),
    McpMessage { message: Value },
    Error { error: String },
}

/// A control command sent from the SDK to the CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKControlCommand {
    #[serde(rename = "type")]
    pub command_type: String,
    #[serde(flatten)]
    pub params: Value,
}

impl SDKControlCommand {
    pub fn interrupt() -> Self {
        Self {
            command_type: "interrupt".into(),
            params: Value::Object(Default::default()),
        }
    }

    pub fn set_permission_mode(mode: &str) -> Self {
        Self {
            command_type: "set_permission_mode".into(),
            params: serde_json::json!({ "mode": mode }),
        }
    }

    pub fn set_model(model: &str) -> Self {
        Self {
            command_type: "set_model".into(),
            params: serde_json::json!({ "model": model }),
        }
    }

    pub fn rewind_files(user_message_id: &str) -> Self {
        Self {
            command_type: "rewind_files".into(),
            params: serde_json::json!({ "user_message_id": user_message_id }),
        }
    }

    pub fn get_mcp_status() -> Self {
        Self {
            command_type: "get_mcp_status".into(),
            params: Value::Object(Default::default()),
        }
    }
}

/// Init handshake message sent from the SDK to the CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKInitMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub protocol_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<SDKCapabilities>,
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

impl SDKInitMessage {
    pub fn new(capabilities: SDKCapabilities) -> Self {
        Self {
            msg_type: "sdk_init".into(),
            protocol_version: "1".into(),
            capabilities: Some(capabilities),
        }
    }
}

/// Init response from the CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKInitResponse {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub protocol_version: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}
