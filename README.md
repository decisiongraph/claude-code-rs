# claude-code-rs

Rust rewrite of the official [Anthropic Claude Agent SDK (Python)](https://github.com/anthropics/claude-agent-sdk-python).

Wraps the Claude Code CLI via subprocess with a bidirectional JSON streaming protocol. Full async, typed messages, hooks, permissions, and in-process MCP server support.

## Requirements

- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) >= 2.0.0 installed and authenticated
- Rust 1.70+

## Quick Start

```toml
[dependencies]
claude-code-rs = "0.1"
tokio = { version = "1", features = ["full"] }
```

```rust
use claude_code_rs::{query_text, ClaudeAgentOptions, PermissionMode};

#[tokio::main]
async fn main() -> claude_code_rs::Result<()> {
    let options = ClaudeAgentOptions {
        permission_mode: PermissionMode::AcceptAll,
        max_turns: Some(1),
        ..Default::default()
    };

    let response = query_text("What is 2+2? Reply with just the number.", options).await?;
    println!("{response}");
    Ok(())
}
```

## APIs

### One-shot query

```rust
// Stream messages
let mut stream = claude_code_rs::query("prompt", options).await?;

// Collect all messages
let messages = claude_code_rs::query_collect("prompt", options).await?;

// Get text only
let text = claude_code_rs::query_text("prompt", options).await?;
```

### Stateful client (multi-turn)

```rust
use claude_code_rs::{ClaudeSDKClient, ClaudeAgentOptions, Message};
use tokio_stream::StreamExt;

let mut client = ClaudeSDKClient::new(ClaudeAgentOptions::default());
client.connect(None).await?;

client.query("What is Rust?", None).await?;
let messages = client.receive_response().await?;

client.query("How does ownership work?", None).await?;
let messages = client.receive_response().await?;

client.disconnect().await?;
```

### Hooks

```rust
use claude_code_rs::*;
use claude_code_rs::types::hooks::HookInput;

let hook = HookDefinition {
    event: HookEvent::PreToolUse,
    matcher: HookMatcher { tool_name: Some("Bash".into()) },
    callback: hook_callback(|input| async move {
        if let HookInput::PreToolUse(pre) = &input {
            let cmd = pre.tool_input.get("command")
                .and_then(|v| v.as_str()).unwrap_or("");
            if cmd.contains("rm ") {
                return HookOutput::block("rm not allowed");
            }
        }
        HookOutput::approve()
    }),
};

let options = ClaudeAgentOptions {
    hooks: vec![hook],
    ..Default::default()
};
```

### In-process MCP tools

```rust
use claude_code_rs::*;

let tool = new_tool(
    "add", "Add two numbers",
    serde_json::json!({"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number"}}}),
    |input| async move {
        let a = input.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let b = input.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
        McpToolResult::text(format!("{}", a + b))
    },
);

let server = create_sdk_mcp_server(vec![tool]);
let mut client = ClaudeSDKClient::new(ClaudeAgentOptions::default());
client.add_mcp_server(server);
```

## Architecture

Mirrors the Python SDK's architecture:

| Layer | Python | Rust |
|-------|--------|------|
| Transport | `SubprocessCLITransport` | `SubprocessTransport` + `TransportWriter` |
| Protocol | `Query` | `Query` (spawn_router task) |
| Messages | dataclasses | enums + serde |
| One-shot API | `query()` | `query()` / `query_text()` / `query_collect()` |
| Stateful API | `ClaudeSDKClient` | `ClaudeSDKClient` |
| MCP | `SdkMcpServer` | `SdkMcpServer` + JSONRPC router |

All communication is newline-delimited JSON over stdin/stdout with a bidirectional control protocol for hooks, permissions, MCP routing, and interrupts.

## Examples

```sh
cargo run --example quick_start
cargo run --example streaming
cargo run --example hooks
cargo run --example mcp_calculator
```

## License

MIT - same as the [original Python SDK](https://github.com/anthropics/claude-agent-sdk-python).
