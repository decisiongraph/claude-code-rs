use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::message_parser::parse_message;
use crate::types::control::{SDKCapabilities, SDKControlCommand, SDKInitMessage};
use crate::types::hooks::{
    HookDecision, HookDefinition, HookEvent, HookInput, NotificationInput, PostToolUseInput,
    PreToolUseInput, StopInput,
};
use crate::types::messages::Message;
use crate::types::permissions::{CanUseToolCallback, CanUseToolInput};
use crate::transport::{Transport, TransportWriter};

const DEFAULT_CONTROL_TIMEOUT: Duration = Duration::from_secs(30);

/// Handler for MCP messages routed through the control protocol.
pub type McpMessageHandler = Arc<
    dyn Fn(String, Value) -> std::pin::Pin<Box<dyn std::future::Future<Output = Value> + Send>>
        + Send
        + Sync,
>;

/// Query manages the bidirectional control protocol over a Transport connection.
///
/// Routes incoming messages: control requests are handled internally,
/// regular messages are forwarded to the consumer channel.
pub struct Query {
    transport: Box<dyn Transport>,
    writer: Option<TransportWriter>,
    hooks: Vec<HookDefinition>,
    can_use_tool: Option<CanUseToolCallback>,
    mcp_handler: Option<McpMessageHandler>,
    pending_responses: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    cancel: CancellationToken,
    control_timeout: Duration,
    server_info: Arc<Mutex<Option<Value>>>,
}

impl Query {
    pub fn new(
        transport: Box<dyn Transport>,
        hooks: Vec<HookDefinition>,
        can_use_tool: Option<CanUseToolCallback>,
        mcp_handler: Option<McpMessageHandler>,
        control_timeout: Option<Duration>,
    ) -> Self {
        Self {
            transport,
            writer: None,
            hooks,
            can_use_tool,
            mcp_handler,
            pending_responses: Arc::new(Mutex::new(HashMap::new())),
            cancel: CancellationToken::new(),
            control_timeout: control_timeout.unwrap_or(DEFAULT_CONTROL_TIMEOUT),
            server_info: Arc::new(Mutex::new(None)),
        }
    }

    /// Connect to the CLI and perform the initialization handshake.
    pub async fn connect(&mut self) -> Result<mpsc::Receiver<Result<Message>>> {
        let (raw_rx, writer) = self.transport.connect().await?;
        self.writer = Some(writer.clone());

        let (consumer_tx, consumer_rx) = mpsc::channel::<Result<Message>>(256);

        // Start the message router task.
        self.spawn_router(raw_rx, consumer_tx, writer.clone());

        // Perform init handshake.
        self.initialize().await?;

        Ok(consumer_rx)
    }

    /// Send a user message to the CLI.
    pub async fn send_message(&self, prompt: &str, session_id: Option<&str>) -> Result<()> {
        let writer = self.writer.as_ref().ok_or(Error::NotConnected)?;
        let msg = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": prompt
            },
            "session_id": session_id.unwrap_or(""),
            "parent_tool_use_id": null
        });
        writer.write(msg).await
    }

    /// Send a control command and wait for the response.
    pub async fn send_control_command(&self, command: SDKControlCommand) -> Result<Value> {
        let writer = self.writer.as_ref().ok_or(Error::NotConnected)?;
        let request_id = generate_request_id();

        let mut request = serde_json::json!({
            "type": "control_request",
            "request_id": request_id,
            "request": {
                "subtype": command.command_type,
            }
        });

        if let Value::Object(params) = command.params {
            if let Value::Object(ref mut req) = request["request"] {
                for (k, v) in params {
                    req.insert(k, v);
                }
            }
        }

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_responses.lock().await;
            pending.insert(request_id.clone(), tx);
        }

        writer.write(request).await?;

        let response = tokio::time::timeout(self.control_timeout, rx)
            .await
            .map_err(|_| Error::ControlTimeout(self.control_timeout))?
            .map_err(|_| Error::ControlProtocol("response channel dropped".into()))?;

        Ok(response)
    }

    pub async fn interrupt(&self) -> Result<Value> {
        self.send_control_command(SDKControlCommand::interrupt())
            .await
    }

    pub async fn set_permission_mode(&self, mode: &str) -> Result<Value> {
        self.send_control_command(SDKControlCommand::set_permission_mode(mode))
            .await
    }

    pub async fn set_model(&self, model: &str) -> Result<Value> {
        self.send_control_command(SDKControlCommand::set_model(model))
            .await
    }

    pub async fn rewind_files(&self, user_message_id: &str) -> Result<Value> {
        self.send_control_command(SDKControlCommand::rewind_files(user_message_id))
            .await
    }

    pub async fn get_mcp_status(&self) -> Result<Value> {
        self.send_control_command(SDKControlCommand::get_mcp_status())
            .await
    }

    pub async fn get_server_info(&self) -> Option<Value> {
        self.server_info.lock().await.clone()
    }

    pub async fn end_input(&self) -> Result<()> {
        self.transport.end_input().await
    }

    pub async fn close(&mut self) -> Result<()> {
        self.cancel.cancel();
        self.writer = None;
        self.transport.close().await
    }

    async fn initialize(&self) -> Result<()> {
        let writer = self.writer.as_ref().ok_or(Error::NotConnected)?;

        let capabilities = SDKCapabilities {
            hooks: !self.hooks.is_empty(),
            permissions: self.can_use_tool.is_some(),
            mcp: self.mcp_handler.is_some(),
            agent_definitions: vec![],
            mcp_servers: vec![],
        };

        let init_msg = SDKInitMessage::new(capabilities);
        let init_value = serde_json::to_value(&init_msg)?;

        let request_id = generate_request_id();
        let request = serde_json::json!({
            "type": "control_request",
            "request_id": request_id,
            "request": {
                "subtype": "initialize",
                "protocol_version": "1",
                "capabilities": init_value.get("capabilities"),
            }
        });

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_responses.lock().await;
            pending.insert(request_id.clone(), tx);
        }

        writer.write(request).await?;

        let response = tokio::time::timeout(self.control_timeout, rx)
            .await
            .map_err(|_| Error::ControlTimeout(self.control_timeout))?
            .map_err(|_| Error::ControlProtocol("init response channel dropped".into()))?;

        {
            let mut info = self.server_info.lock().await;
            *info = Some(response);
        }

        Ok(())
    }

    fn spawn_router(
        &self,
        mut raw_rx: mpsc::Receiver<Result<Value>>,
        consumer_tx: mpsc::Sender<Result<Message>>,
        writer: TransportWriter,
    ) {
        let pending = self.pending_responses.clone();
        let hooks = self.hooks.clone();
        let can_use_tool = self.can_use_tool.clone();
        let mcp_handler = self.mcp_handler.clone();
        let cancel = self.cancel.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    msg = raw_rx.recv() => {
                        match msg {
                            Some(Ok(value)) => {
                                let msg_type = value.get("type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");

                                match msg_type {
                                    "control_response" => {
                                        route_control_response(&pending, &value).await;
                                    }
                                    "control_request" => {
                                        dispatch_control_request(
                                            &value,
                                            &hooks,
                                            &can_use_tool,
                                            &mcp_handler,
                                            &writer,
                                        ).await;
                                    }
                                    _ => {
                                        let parsed = parse_message(value);
                                        if consumer_tx.send(parsed).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                            Some(Err(e)) => {
                                let _ = consumer_tx.send(Err(e)).await;
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }
        });
    }
}

async fn route_control_response(
    pending: &Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    value: &Value,
) {
    let response = value.get("response").cloned().unwrap_or(value.clone());
    let request_id = response
        .get("request_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut pending = pending.lock().await;
    if let Some(tx) = pending.remove(request_id) {
        let _ = tx.send(response);
    } else {
        tracing::warn!(request_id, "control response for unknown request");
    }
}

async fn dispatch_control_request(
    value: &Value,
    hooks: &[HookDefinition],
    can_use_tool: &Option<CanUseToolCallback>,
    mcp_handler: &Option<McpMessageHandler>,
    writer: &TransportWriter,
) {
    let request_id = value
        .get("request_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let request = match value.get("request") {
        Some(r) => r,
        None => {
            tracing::warn!("control request missing 'request' field");
            return;
        }
    };

    let subtype = request
        .get("subtype")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let response_body = match subtype {
        "can_use_tool" => handle_can_use_tool(request, can_use_tool).await,
        "hook_callback" => handle_hook_callback(request, hooks).await,
        "mcp_message" => handle_mcp_message(request, mcp_handler).await,
        other => {
            tracing::warn!(subtype = other, "unknown control request subtype");
            serde_json::json!({"error": format!("unknown subtype: {other}")})
        }
    };

    let control_response = serde_json::json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": response_body,
        }
    });

    if let Err(e) = writer.write(control_response).await {
        tracing::error!("failed to send control response: {e}");
    }
}

async fn handle_can_use_tool(request: &Value, callback: &Option<CanUseToolCallback>) -> Value {
    let tool_name = request
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let input = request.get("input").cloned().unwrap_or(Value::Null);

    if let Some(cb) = callback {
        let result = cb(CanUseToolInput { tool_name, input }).await;
        if result.allowed {
            serde_json::json!({"behavior": "allow"})
        } else {
            serde_json::json!({
                "behavior": "deny",
                "message": result.reason.unwrap_or_default()
            })
        }
    } else {
        serde_json::json!({"behavior": "allow"})
    }
}

async fn handle_hook_callback(request: &Value, hooks: &[HookDefinition]) -> Value {
    let callback_id = request
        .get("callback_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let hook_input = request.get("input").cloned().unwrap_or(Value::Null);

    let hook_index: Option<usize> = callback_id
        .strip_prefix("hook_")
        .and_then(|s| s.parse().ok());

    let hook = hook_index.and_then(|i| hooks.get(i));

    if let Some(hook) = hook {
        let typed_input = match hook.event {
            HookEvent::PreToolUse => {
                let pre: PreToolUseInput =
                    serde_json::from_value(hook_input).unwrap_or(PreToolUseInput {
                        tool_name: String::new(),
                        tool_input: Value::Null,
                    });
                HookInput::PreToolUse(pre)
            }
            HookEvent::PostToolUse => {
                let post: PostToolUseInput =
                    serde_json::from_value(hook_input).unwrap_or(PostToolUseInput {
                        tool_name: String::new(),
                        tool_input: Value::Null,
                        tool_output: Value::Null,
                    });
                HookInput::PostToolUse(post)
            }
            HookEvent::Notification => {
                let notif: NotificationInput =
                    serde_json::from_value(hook_input).unwrap_or(NotificationInput {
                        title: String::new(),
                        message: None,
                    });
                HookInput::Notification(notif)
            }
            HookEvent::Stop | HookEvent::SubagentStop => {
                let stop: StopInput =
                    serde_json::from_value(hook_input).unwrap_or(StopInput { reason: None });
                HookInput::Stop(stop)
            }
        };

        let output = (hook.callback)(typed_input).await;
        let mut result = serde_json::json!({"continue": true});
        if let Some(decision) = &output.decision {
            let hook_specific = serde_json::json!({
                "hookEventName": match hook.event {
                    HookEvent::PreToolUse => "PreToolUse",
                    HookEvent::PostToolUse => "PostToolUse",
                    HookEvent::Notification => "Notification",
                    HookEvent::Stop => "Stop",
                    HookEvent::SubagentStop => "SubagentStop",
                },
                "permissionDecision": match decision {
                    HookDecision::Approve => "approve",
                    HookDecision::Block => "deny",
                    HookDecision::Ignore => "ignore",
                },
                "permissionDecisionReason": output.reason.as_deref().unwrap_or(""),
            });
            result["hookSpecificOutput"] = hook_specific;

            if *decision == HookDecision::Block {
                result["continue"] = Value::Bool(false);
            }
        }
        result
    } else {
        tracing::warn!(callback_id, "hook callback not found");
        serde_json::json!({"continue": true})
    }
}

async fn handle_mcp_message(request: &Value, handler: &Option<McpMessageHandler>) -> Value {
    let server_name = request
        .get("server_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let message = request.get("message").cloned().unwrap_or(Value::Null);

    if let Some(handler) = handler {
        handler(server_name, message).await
    } else {
        serde_json::json!({"error": "no MCP handler registered"})
    }
}

fn generate_request_id() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let suffix: u64 = rng.random();
    format!("req_{suffix:016x}")
}

impl Drop for Query {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
