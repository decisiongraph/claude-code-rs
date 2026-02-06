use claude_code_rs::{
    new_tool, create_sdk_mcp_server, ClaudeAgentOptions, ClaudeSDKClient, McpToolResult, Message,
    PermissionMode,
};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> claude_code_rs::Result<()> {
    // Define an in-process MCP calculator tool.
    let add_tool = new_tool(
        "add",
        "Add two numbers together",
        serde_json::json!({
            "type": "object",
            "properties": {
                "a": {"type": "number", "description": "First number"},
                "b": {"type": "number", "description": "Second number"}
            },
            "required": ["a", "b"]
        }),
        |input| async move {
            let a = input.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let b = input.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
            McpToolResult::text(format!("{}", a + b))
        },
    );

    let multiply_tool = new_tool(
        "multiply",
        "Multiply two numbers",
        serde_json::json!({
            "type": "object",
            "properties": {
                "a": {"type": "number"},
                "b": {"type": "number"}
            },
            "required": ["a", "b"]
        }),
        |input| async move {
            let a = input.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let b = input.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
            McpToolResult::text(format!("{}", a * b))
        },
    );

    let server = create_sdk_mcp_server(vec![add_tool, multiply_tool]);

    let options = ClaudeAgentOptions {
        permission_mode: PermissionMode::AcceptAll,
        max_turns: Some(5),
        ..Default::default()
    };

    let mut client = ClaudeSDKClient::new(options);
    client.add_mcp_server(server);
    client.connect(None).await?;

    client
        .query("What is (12 + 8) * 3? Use the calculator tools.", None)
        .await?;

    let mut stream = client.receive_messages();
    while let Some(msg) = stream.next().await {
        match msg? {
            Message::Assistant { message } => {
                for block in &message.content {
                    if let Some(text) = block.as_text() {
                        print!("{text}");
                    }
                }
            }
            Message::Result { .. } => {
                println!("\n[done]");
                break;
            }
            _ => {}
        }
    }

    drop(stream);
    client.disconnect().await?;
    Ok(())
}
