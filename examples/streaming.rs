use claude_code_rs::{query, ClaudeAgentOptions, Message, PermissionMode};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> claude_code_rs::Result<()> {
    let options = ClaudeAgentOptions {
        permission_mode: PermissionMode::AcceptAll,
        max_turns: Some(3),
        ..Default::default()
    };

    let mut stream = query("List 5 programming languages and one sentence about each.", options).await?;

    while let Some(msg) = stream.next().await {
        match msg? {
            Message::Assistant { message } => {
                for block in &message.content {
                    if let Some(text) = block.as_text() {
                        print!("{text}");
                    }
                }
            }
            Message::Result { result } => {
                println!("\n---");
                if let Some(cost) = result.total_cost_usd {
                    println!("Cost: ${cost:.4}");
                }
                if let Some(turns) = result.num_turns {
                    println!("Turns: {turns}");
                }
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
