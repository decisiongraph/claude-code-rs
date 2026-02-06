use serde_json::Value;

use crate::error::{Error, Result};
use crate::types::messages::{AssistantMessage, Message, ResultMessage, UserMessage};

/// Parse a raw JSON value from the CLI stream into a typed Message.
///
/// The CLI emits newline-delimited JSON with a top-level `type` field.
/// Each type has a different structure:
/// - `"assistant"` and `"user"`: have a nested `"message"` object
/// - `"result"`: top-level fields (no message wrapper)
/// - `"system"`: has a `"subtype"` field
/// - Others: preserved as Unknown
pub fn parse_message(raw: Value) -> Result<Message> {
    let msg_type = raw
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::MessageParse {
            reason: "missing 'type' field".into(),
        })?;

    match msg_type {
        "assistant" => parse_assistant(raw),
        "user" => parse_user(raw),
        "result" => parse_result(raw),
        "system" => parse_system(raw),
        other => Ok(Message::Unknown {
            message_type: other.to_string(),
            raw,
        }),
    }
}

fn parse_assistant(raw: Value) -> Result<Message> {
    // The assistant message body is in the "message" field.
    let message_value = raw.get("message").cloned().unwrap_or(raw.clone());

    let message: AssistantMessage =
        serde_json::from_value(message_value).map_err(|e| Error::MessageParse {
            reason: format!("assistant message parse failed: {e}"),
        })?;

    Ok(Message::Assistant { message })
}

fn parse_user(raw: Value) -> Result<Message> {
    let message_value = raw.get("message").cloned().unwrap_or(raw.clone());

    let message: UserMessage =
        serde_json::from_value(message_value).map_err(|e| Error::MessageParse {
            reason: format!("user message parse failed: {e}"),
        })?;

    Ok(Message::User { message })
}

fn parse_result(raw: Value) -> Result<Message> {
    // Result messages have their fields at the top level (no "message" wrapper).
    let result: ResultMessage =
        serde_json::from_value(raw).map_err(|e| Error::MessageParse {
            reason: format!("result message parse failed: {e}"),
        })?;

    Ok(Message::Result { result })
}

fn parse_system(raw: Value) -> Result<Message> {
    let subtype = raw
        .get("subtype")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(Message::System {
        subtype,
        data: raw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_assistant_message() {
        let raw = serde_json::json!({
            "type": "assistant",
            "message": {
                "model": "claude-sonnet-4-5",
                "content": [
                    {"type": "text", "text": "Hello!"}
                ]
            }
        });
        let msg = parse_message(raw).unwrap();
        assert!(matches!(msg, Message::Assistant { .. }));
        assert_eq!(msg.text().unwrap(), "Hello!");
    }

    #[test]
    fn parse_user_message() {
        let raw = serde_json::json!({
            "type": "user",
            "message": {
                "content": "What is 2+2?"
            }
        });
        let msg = parse_message(raw).unwrap();
        assert!(matches!(msg, Message::User { .. }));
    }

    #[test]
    fn parse_result_message() {
        let raw = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "duration_ms": 1234.0,
            "num_turns": 3,
            "session_id": "sess_123",
            "total_cost_usd": 0.05
        });
        let msg = parse_message(raw).unwrap();
        assert!(msg.is_result());
        assert!(!msg.is_error());
        assert_eq!(msg.session_id(), Some("sess_123"));
    }

    #[test]
    fn parse_system_message() {
        let raw = serde_json::json!({
            "type": "system",
            "subtype": "init",
            "data": {"version": "2.1.0"}
        });
        let msg = parse_message(raw).unwrap();
        match msg {
            Message::System { subtype, .. } => assert_eq!(subtype, "init"),
            _ => panic!("expected System"),
        }
    }

    #[test]
    fn parse_unknown_type() {
        let raw = serde_json::json!({
            "type": "stream_event",
            "event": {"delta": "hello"}
        });
        let msg = parse_message(raw).unwrap();
        match msg {
            Message::Unknown { message_type, .. } => assert_eq!(message_type, "stream_event"),
            _ => panic!("expected Unknown"),
        }
    }

    #[test]
    fn parse_missing_type() {
        let raw = serde_json::json!({"data": "oops"});
        assert!(parse_message(raw).is_err());
    }

    #[test]
    fn parse_assistant_with_tool_use() {
        let raw = serde_json::json!({
            "type": "assistant",
            "message": {
                "model": "claude-sonnet-4-5",
                "content": [
                    {"type": "text", "text": "Let me run that."},
                    {"type": "tool_use", "id": "tu_1", "name": "Bash", "input": {"command": "ls"}}
                ]
            }
        });
        let msg = parse_message(raw).unwrap();
        if let Message::Assistant { message } = msg {
            assert_eq!(message.content.len(), 2);
        } else {
            panic!("expected Assistant");
        }
    }
}
