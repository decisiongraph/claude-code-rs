# Principal Engineer Review: claude-code-rs

## Executive Summary

The `claude-code-rs` library is a robust, idiomatic Rust implementation of the Anthropic Claude Agent SDK. It successfully mirrors the architecture of the Python SDK while leveraging Rust's type safety and async capabilities (Tokio). The codebase is clean, well-organized, and demonstrates a good understanding of concurrent systems.

However, there is **one critical usability flaw in the `ClaudeSDKClient` API** that can lead to broken client states if not handled correctly by the consumer. Addressing this, along with a few minor robustness improvements, will make this a production-ready library.

## Architecture & Code Quality

### Strengths
- **Async/Await Correctness**: The use of `tokio::spawn` for the router and transport tasks is correct and efficient. `tokio::select!` is used effectively for cancellation and multiplexing.
- **Type Safety**: The strong typing of messages (`Message`, `AssistantMessage`, `UserMessage`) and control commands provides a significant developer experience upgrade over loosely typed JSON.
- **Separation of Concerns**: The split between `Transport` (subprocess IO), `Query` (protocol logic), and `ClaudeSDKClient` (state management) is clean and maintainable.
- **Error Handling**: The `Result` type is consistently used. Custom error types in `error.rs` cover the domain well.

### Architecture Analysis
The architecture faithfully replicates the Python SDK's design. The "Router" pattern in `Query::spawn_router` effectively manages the complexity of the bidirectional protocol, handling control requests (like hooks and tools) independently of the main message flow.

## Critical Findings

### 1. `ClaudeSDKClient::receive_messages` Ownership Trap
**Severity:** High
**Location:** `src/client.rs`

The `receive_messages` method permanently removes the internal `mpsc::Receiver` from the client:

```rust
pub fn receive_messages(&mut self) -> ReceiverStream<Result<Message>> {
    let rx = self.message_rx.take().unwrap_or_else(|| ...);
    ReceiverStream::new(rx)
}
```

If a user calls this method to stream responses for a query, the `Receiver` is moved out of the `ClaudeSDKClient`. Unless the user manually extracts the receiver implementation and puts it back (which the API does not currently support via a public setter), **the client becomes unusable for receiving future messages**.

The `receive_response` helper handles this correctly by rebuilding the stream, but the exposed `receive_messages` API is a "footgun".

**Recommendation:**
Change `receive_messages` to not consume the receiver permanently, or provide a way to borrow the stream. Alternatively, wrap the receiver in a custom struct that returns it to the client on drop, or simply guard against this usage pattern.

## Minor Issues & Observations

### 2. Silent Hook Input Parsing Failures
**Severity:** Low
**Location:** `src/query.rs`

In `handle_hook_callback`, input parsing failures default to a zero-value:
```rust
HookInput::PreToolUse(serde_json::from_value(hook_input).unwrap_or_default())
```
If the CLI sends malformed inputs or the schema changes, the hook will be called with an empty/default structure rather than handling the error. This might lead to confusing behavior in user hooks.

**Recommendation:**
Log a warning if parsing fails, or return an error in the control response to the CLI.

### 3. MCP Server Registration Timing
**Severity:** Low
**Location:** `src/client.rs`

MCP servers are frozen at connection time. `build_mcp_handler` clones the server map:
```rust
let servers = self.mcp_servers.clone();
```
Calling `add_mcp_server` after `connect()` will modify the client's map but will not affect the active connection.

**Recommendation:**
Document this behavior clearly, or use a shared `Arc<RwLock<HashMap...>>` caught in the closure to allow dynamic registration (if thread-safety permits).

### 4. JSON parsing robustness
**Severity:** Info
**Location:** `src/transport/subprocess.rs`

The transport logs a warning on JSON parse failures and continues. This is generally good for resilience against stdout noise, but ensure that critical protocol errors aren't being swallowed.

## Conclusion

`claude-code-rs` is a high-quality implementation. The core transport and protocol logic are solid. Fixing the `receive_messages` API life-cycle issue is the only major hurdle to it being a highly reliable SDK.

**Action Items:**
1.  Refactor `receive_messages` / `message_rx` management.
2.  Add error logging to hook input parsing.
3.  Add doc comments clarifying MCP registration timing.
