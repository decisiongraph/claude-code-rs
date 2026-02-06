use serde_json::Value;

/// Route a JSONRPC request to the appropriate handler.
pub fn route_jsonrpc(
    request: &Value,
    tools: &[super::server::McpTool],
) -> Option<JsonRpcAction> {
    let method = request.get("method")?.as_str()?;
    let id = request.get("id").cloned();

    match method {
        "initialize" => Some(JsonRpcAction::Response {
            id,
            result: serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "claude-agent-sdk-rs",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        }),

        "notifications/initialized" => {
            // No response needed for notifications.
            Some(JsonRpcAction::None)
        }

        "tools/list" => {
            let tools_list: Vec<Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "inputSchema": t.input_schema,
                    })
                })
                .collect();

            Some(JsonRpcAction::Response {
                id,
                result: serde_json::json!({ "tools": tools_list }),
            })
        }

        "tools/call" => {
            let params = request.get("params")?;
            let tool_name = params.get("name")?.as_str()?.to_string();
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Object(Default::default()));

            Some(JsonRpcAction::ToolCall {
                id,
                tool_name,
                arguments,
            })
        }

        _ => Some(JsonRpcAction::Error {
            id,
            code: -32601,
            message: format!("method not found: {method}"),
        }),
    }
}

/// Action to take after routing a JSONRPC request.
pub enum JsonRpcAction {
    /// Send a response immediately.
    Response { id: Option<Value>, result: Value },
    /// Call a tool (async), then send response.
    ToolCall {
        id: Option<Value>,
        tool_name: String,
        arguments: Value,
    },
    /// Send an error response.
    Error {
        id: Option<Value>,
        code: i64,
        message: String,
    },
    /// No response needed (notifications).
    None,
}

/// Build a JSONRPC success response.
pub fn jsonrpc_response(id: Option<Value>, result: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

/// Build a JSONRPC error response.
pub fn jsonrpc_error(id: Option<Value>, code: i64, message: &str) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_initialize() {
        let req = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
        let action = route_jsonrpc(&req, &[]).unwrap();
        assert!(matches!(action, JsonRpcAction::Response { .. }));
    }

    #[test]
    fn route_tools_list() {
        let tool = super::super::server::McpTool {
            name: "calc".into(),
            description: "calculator".into(),
            input_schema: serde_json::json!({"type": "object"}),
            handler: super::super::server::noop_handler(),
        };
        let req = serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        let action = route_jsonrpc(&req, &[tool]).unwrap();
        match action {
            JsonRpcAction::Response { result, .. } => {
                let tools = result["tools"].as_array().unwrap();
                assert_eq!(tools.len(), 1);
                assert_eq!(tools[0]["name"], "calc");
            }
            _ => panic!("expected Response"),
        }
    }

    #[test]
    fn route_tools_call() {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "calc", "arguments": {"a": 1}}
        });
        let action = route_jsonrpc(&req, &[]).unwrap();
        match action {
            JsonRpcAction::ToolCall { tool_name, arguments, .. } => {
                assert_eq!(tool_name, "calc");
                assert_eq!(arguments["a"], 1);
            }
            _ => panic!("expected ToolCall"),
        }
    }

    #[test]
    fn route_unknown_method() {
        let req = serde_json::json!({"jsonrpc": "2.0", "id": 4, "method": "foo/bar"});
        let action = route_jsonrpc(&req, &[]).unwrap();
        assert!(matches!(action, JsonRpcAction::Error { .. }));
    }
}
