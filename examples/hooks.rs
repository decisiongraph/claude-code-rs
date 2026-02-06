use claude_code_rs::{
    hook_callback, query, ClaudeAgentOptions, HookDefinition, HookEvent, HookMatcher, HookOutput,
    Message, PermissionMode,
};
use claude_code_rs::types::hooks::HookInput;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> claude_code_rs::Result<()> {
    // Hook that blocks rm/rmdir commands in Bash tool.
    let block_dangerous = HookDefinition {
        event: HookEvent::PreToolUse,
        matcher: HookMatcher {
            tool_name: Some("Bash".into()),
        },
        callback: hook_callback(|input| async move {
            if let HookInput::PreToolUse(pre) = &input {
                let cmd = pre
                    .tool_input
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if cmd.contains("rm ") || cmd.contains("rmdir") {
                    println!("[HOOK] Blocked dangerous command: {cmd}");
                    return HookOutput::block("rm/rmdir commands are not allowed");
                }
            }
            HookOutput::approve()
        }),
    };

    let options = ClaudeAgentOptions {
        permission_mode: PermissionMode::AcceptAll,
        max_turns: Some(3),
        hooks: vec![block_dangerous],
        ..Default::default()
    };

    let mut stream = query("Create a temp file, then try to delete it with rm.", options).await?;

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

    Ok(())
}
