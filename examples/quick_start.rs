use claude_code_rs::{query_text, ClaudeAgentOptions, PermissionMode};

#[tokio::main]
async fn main() -> claude_code_rs::Result<()> {
    let options = ClaudeAgentOptions {
        permission_mode: PermissionMode::AcceptAll,
        max_turns: Some(1),
        ..Default::default()
    };

    let response = query_text("What is 2+2? Reply with just the number.", options).await?;
    println!("Claude says: {response}");
    Ok(())
}
