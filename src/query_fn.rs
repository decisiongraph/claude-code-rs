use tokio_stream::wrappers::ReceiverStream;

use crate::error::{Error, Result};
use crate::query::Query;
use crate::transport::cli_discovery;
use crate::transport::subprocess::SubprocessTransport;
use crate::types::messages::Message;
use crate::types::options::ClaudeAgentOptions;

/// Execute a one-shot query against the Claude CLI and return a stream of messages.
///
/// This is the simplest API - it handles connection, init handshake, sending the prompt,
/// and returns a stream of messages until the result message arrives.
///
/// # Example
/// ```no_run
/// use claude_code_rs::{ClaudeAgentOptions, Message};
/// use claude_code_rs::query_fn::query;
/// use tokio_stream::StreamExt;
///
/// # async fn example() -> claude_code_rs::Result<()> {
/// let options = ClaudeAgentOptions {
///     max_turns: Some(3),
///     ..Default::default()
/// };
///
/// let mut stream = query("What is 2+2?", options).await?;
/// while let Some(msg) = stream.next().await {
///     match msg? {
///         Message::Assistant { message } => {
///             if let Some(text) = message.content.iter()
///                 .find_map(|b| b.as_text()) {
///                 print!("{text}");
///             }
///         }
///         Message::Result { result } => {
///             println!("\n[done, cost: {:?}]", result.total_cost_usd);
///             break;
///         }
///         _ => {}
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub async fn query(
    prompt: &str,
    options: ClaudeAgentOptions,
) -> Result<ReceiverStream<Result<Message>>> {
    let cli_path = match options.cli_path {
        Some(ref p) => p.clone(),
        None => cli_discovery::find_cli()?,
    };

    let transport = SubprocessTransport::new(cli_path, &options);
    let mut q = Query::new(
        Box::new(transport),
        options.hooks,
        options.can_use_tool,
        None, // MCP handler wired through client, not one-shot query
        options.control_timeout,
    );

    let rx = q.connect().await?;

    // Send the prompt.
    q.send_message(prompt, None).await?;

    // Return the message stream. The Query and transport are kept alive
    // by the spawned tasks until the channel is dropped.
    // We leak the Query intentionally - it will be cleaned up when
    // the spawned tasks finish (transport closes, channels drop).
    // This matches the Python SDK's query() behavior.
    std::mem::forget(q);

    Ok(ReceiverStream::new(rx))
}

/// Execute a query and collect all messages until the result.
///
/// Returns the full list of messages including the final ResultMessage.
pub async fn query_collect(
    prompt: &str,
    options: ClaudeAgentOptions,
) -> Result<Vec<Message>> {
    use tokio_stream::StreamExt;

    let mut stream = query(prompt, options).await?;
    let mut messages = Vec::new();

    while let Some(msg) = stream.next().await {
        let msg = msg?;
        let is_result = msg.is_result();
        messages.push(msg);
        if is_result {
            break;
        }
    }

    Ok(messages)
}

/// Execute a query and return just the text response.
///
/// Collects all assistant text blocks and joins them.
pub async fn query_text(
    prompt: &str,
    options: ClaudeAgentOptions,
) -> Result<String> {
    let messages = query_collect(prompt, options).await?;
    let mut text = String::new();

    for msg in &messages {
        if let Some(t) = msg.text() {
            text.push_str(&t);
        }
    }

    if text.is_empty() {
        // Check if there was an error.
        if let Some(Message::Result { result }) = messages.last() {
            if result.is_error {
                return Err(Error::Process(
                    result.error.clone().unwrap_or("unknown error".into()),
                ));
            }
        }
    }

    Ok(text)
}
