use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

use super::jsonrpc::{self, JsonRpcAction};

/// Result of a tool invocation.
#[derive(Debug, Clone)]
pub struct McpToolResult {
    pub content: Vec<McpToolResultContent>,
    pub is_error: bool,
}

#[derive(Debug, Clone)]
pub struct McpToolResultContent {
    pub content_type: String,
    pub text: String,
}

impl McpToolResult {
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![McpToolResultContent {
                content_type: "text".into(),
                text: text.into(),
            }],
            is_error: false,
        }
    }

    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![McpToolResultContent {
                content_type: "text".into(),
                text: message.into(),
            }],
            is_error: true,
        }
    }

    fn to_json(&self) -> Value {
        let content: Vec<Value> = self
            .content
            .iter()
            .map(|c| {
                serde_json::json!({
                    "type": c.content_type,
                    "text": c.text,
                })
            })
            .collect();

        serde_json::json!({
            "content": content,
            "isError": self.is_error,
        })
    }
}

/// Async handler for an MCP tool invocation.
pub type McpToolHandler = Arc<
    dyn Fn(Value) -> Pin<Box<dyn Future<Output = McpToolResult> + Send>> + Send + Sync,
>;

/// An MCP tool definition.
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub handler: McpToolHandler,
}

impl std::fmt::Debug for McpTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpTool")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}

/// Create an McpTool with a typed handler.
pub fn new_tool<F, Fut>(
    name: impl Into<String>,
    description: impl Into<String>,
    input_schema: Value,
    handler: F,
) -> McpTool
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = McpToolResult> + Send + 'static,
{
    McpTool {
        name: name.into(),
        description: description.into(),
        input_schema,
        handler: Arc::new(move |input| Box::pin(handler(input))),
    }
}

/// A no-op handler for testing.
#[cfg(test)]
pub(crate) fn noop_handler() -> McpToolHandler {
    Arc::new(|_| Box::pin(async { McpToolResult::text("noop") }))
}

/// An in-process MCP server that handles JSONRPC messages.
pub struct SdkMcpServer {
    tools: HashMap<String, McpTool>,
}

impl SdkMcpServer {
    #[must_use]
    pub fn new(tools: Vec<McpTool>) -> Self {
        let mut map = HashMap::new();
        for tool in tools {
            map.insert(tool.name.clone(), tool);
        }
        Self { tools: map }
    }

    /// Get the list of tools for tools/list responses.
    pub fn tool_list(&self) -> Vec<&McpTool> {
        self.tools.values().collect()
    }

    /// Handle a JSONRPC message and return the response.
    pub async fn handle_message(&self, message: Value) -> Value {
        let tools_ref: Vec<&McpTool> = self.tools.values().collect();
        let action = match jsonrpc::route_jsonrpc(&message, &tools_ref) {
            Some(action) => action,
            None => {
                return jsonrpc::jsonrpc_error(
                    message.get("id").cloned(),
                    -32600,
                    "invalid request",
                );
            }
        };

        match action {
            JsonRpcAction::Response { id, result } => jsonrpc::jsonrpc_response(id, result),

            JsonRpcAction::ToolCall {
                id,
                tool_name,
                arguments,
            } => {
                if let Some(tool) = self.tools.get(&tool_name) {
                    let result = (tool.handler)(arguments).await;
                    jsonrpc::jsonrpc_response(id, result.to_json())
                } else {
                    jsonrpc::jsonrpc_error(
                        id,
                        -32602,
                        &format!("unknown tool: {tool_name}"),
                    )
                }
            }

            JsonRpcAction::Error { id, code, message } => {
                jsonrpc::jsonrpc_error(id, code, &message)
            }

            JsonRpcAction::None => Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sdk_mcp_server_handles_initialize() {
        let server = SdkMcpServer::new(vec![]);
        let req = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
        let resp = server.handle_message(req).await;
        assert!(resp.get("result").is_some());
        assert_eq!(resp["result"]["capabilities"]["tools"], serde_json::json!({}));
    }

    #[tokio::test]
    async fn sdk_mcp_server_lists_tools() {
        let tool = new_tool("add", "Add two numbers", serde_json::json!({"type": "object"}), |_| async {
            McpToolResult::text("42")
        });
        let server = SdkMcpServer::new(vec![tool]);
        let req = serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        let resp = server.handle_message(req).await;
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "add");
    }

    #[tokio::test]
    async fn sdk_mcp_server_calls_tool() {
        let tool = new_tool(
            "add",
            "Add two numbers",
            serde_json::json!({"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number"}}}),
            |input| async move {
                let a = input.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let b = input.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                McpToolResult::text(format!("{}", a + b))
            },
        );
        let server = SdkMcpServer::new(vec![tool]);
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "add", "arguments": {"a": 2, "b": 3}}
        });
        let resp = server.handle_message(req).await;
        let content = &resp["result"]["content"][0]["text"];
        assert_eq!(content, "5");
    }

    #[tokio::test]
    async fn sdk_mcp_server_unknown_tool() {
        let server = SdkMcpServer::new(vec![]);
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {"name": "missing", "arguments": {}}
        });
        let resp = server.handle_message(req).await;
        assert!(resp.get("error").is_some());
    }
}
