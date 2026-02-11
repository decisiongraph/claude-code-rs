use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::types::options::{ClaudeAgentOptions, StderrCallback};
use crate::types::permissions::PermissionMode;

use super::{Transport, TransportWriter};

/// Transport that communicates with the Claude CLI over WebSocket.
///
/// Binds a local TCP listener, passes `--sdk-url ws://127.0.0.1:{port}`
/// to the CLI, then upgrades the accepted connection to WebSocket.
/// The NDJSON protocol is identical to subprocess — just the transport differs.
pub struct WebSocketTransport {
    cli_path: PathBuf,
    options: BuildOptions,
    child: Option<Child>,
    cancel: CancellationToken,
    ready: bool,
    connect_timeout: Duration,
}

/// Subset of ClaudeAgentOptions needed for building the CLI command.
struct BuildOptions {
    model: Option<String>,
    system_prompt: Option<String>,
    append_system_prompt: Option<String>,
    max_turns: Option<u32>,
    max_tokens: Option<u32>,
    session_id: Option<String>,
    continue_session: bool,
    cwd: Option<PathBuf>,
    permission_mode: PermissionMode,
    allowed_tools: Vec<String>,
    no_cache: bool,
    temperature: Option<f64>,
    context_window: Option<f64>,
    extra_cli_args: Vec<String>,
    env: std::collections::HashMap<String, String>,
    on_stderr: Option<StderrCallback>,
}

impl From<&ClaudeAgentOptions> for BuildOptions {
    fn from(opts: &ClaudeAgentOptions) -> Self {
        Self {
            model: opts.model.clone(),
            system_prompt: opts.system_prompt.clone(),
            append_system_prompt: opts.append_system_prompt.clone(),
            max_turns: opts.max_turns,
            max_tokens: opts.max_tokens,
            session_id: opts.session_id.clone(),
            continue_session: opts.continue_session,
            cwd: opts.cwd.clone(),
            permission_mode: opts.permission_mode.clone(),
            allowed_tools: opts.allowed_tools.clone(),
            no_cache: opts.no_cache,
            temperature: opts.temperature,
            context_window: opts.context_window,
            extra_cli_args: opts.extra_cli_args.clone(),
            env: opts.env.clone(),
            on_stderr: opts.on_stderr.clone(),
        }
    }
}

impl WebSocketTransport {
    pub fn new(cli_path: PathBuf, options: &ClaudeAgentOptions) -> Self {
        let connect_timeout = options
            .connect_timeout
            .unwrap_or(Duration::from_secs(30));
        Self {
            cli_path,
            options: BuildOptions::from(options),
            child: None,
            cancel: CancellationToken::new(),
            ready: false,
            connect_timeout,
        }
    }

    /// Build the CLI command with all flags plus `--sdk-url` for WebSocket.
    fn build_command(&self, port: u16) -> Command {
        let mut cmd = Command::new(&self.cli_path);

        cmd.args(["--output-format", "stream-json"]);
        cmd.args(["--input-format", "stream-json"]);
        cmd.arg("--verbose");
        cmd.arg("--print");
        cmd.args(["--sdk-url", &format!("ws://127.0.0.1:{port}")]);

        if let Some(ref model) = self.options.model {
            cmd.args(["--model", model]);
        }

        if let Some(ref sp) = self.options.system_prompt {
            cmd.args(["--system-prompt", sp]);
        }

        if let Some(ref asp) = self.options.append_system_prompt {
            cmd.args(["--append-system-prompt", asp]);
        }

        if let Some(turns) = self.options.max_turns {
            cmd.args(["--max-turns", &turns.to_string()]);
        }

        if let Some(tokens) = self.options.max_tokens {
            cmd.args(["--max-tokens", &tokens.to_string()]);
        }

        if let Some(ref sid) = self.options.session_id {
            cmd.args(["--session-id", sid]);
        }

        if self.options.continue_session {
            cmd.arg("--continue");
        }

        match &self.options.permission_mode {
            PermissionMode::Default => {}
            PermissionMode::AcceptAll => {
                cmd.args(["--permission-mode", "bypassPermissions"]);
            }
            PermissionMode::DenyAll => {
                cmd.args(["--permission-mode", "plan"]);
            }
            PermissionMode::AllowedTools => {
                for tool in &self.options.allowed_tools {
                    cmd.args(["--allowedTools", tool]);
                }
            }
        }

        if self.options.no_cache {
            cmd.arg("--no-cache");
        }

        if let Some(temp) = self.options.temperature {
            cmd.args(["--temperature", &temp.to_string()]);
        }

        if let Some(cw) = self.options.context_window {
            cmd.args(["--context-window", &cw.to_string()]);
        }

        for arg in &self.options.extra_cli_args {
            cmd.arg(arg);
        }

        if let Some(ref cwd) = self.options.cwd {
            cmd.current_dir(cwd);
        }

        for (key, val) in &self.options.env {
            cmd.env(key, val);
        }

        // With --sdk-url the CLI uses the WebSocket for I/O, not stdin/stdout.
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::piped());

        cmd
    }
}

impl Transport for WebSocketTransport {
    fn connect(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(mpsc::Receiver<Result<Value>>, TransportWriter)>> + Send + '_>>
    {
        Box::pin(self.connect_impl())
    }

    fn end_input(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn close(&mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(self.close_impl())
    }

    fn is_ready(&self) -> bool {
        self.ready
    }
}

impl WebSocketTransport {
    async fn connect_impl(&mut self) -> Result<(mpsc::Receiver<Result<Value>>, TransportWriter)> {
        if self.ready {
            return Err(Error::AlreadyConnected);
        }

        // 1. Bind on random port.
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| Error::WebSocket(format!("failed to bind TCP listener: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| Error::WebSocket(format!("failed to get local addr: {e}")))?
            .port();

        tracing::debug!("WebSocket transport listening on 127.0.0.1:{port}");

        // 2. Build and spawn CLI process.
        let mut cmd = self.build_command(port);
        let mut child = cmd
            .spawn()
            .map_err(|e| Error::CliConnection(format!("failed to spawn CLI: {e}")))?;

        // Grab stderr before storing child.
        let stderr = child.stderr.take();
        self.child = Some(child);

        // 3. Accept TCP connection with timeout.
        let tcp_stream = tokio::time::timeout(self.connect_timeout, listener.accept())
            .await
            .map_err(|_| {
                Error::WebSocket(format!(
                    "timed out waiting for CLI to connect ({}s)",
                    self.connect_timeout.as_secs()
                ))
            })?
            .map_err(|e| Error::WebSocket(format!("TCP accept failed: {e}")))?
            .0;

        tracing::debug!("CLI connected via TCP, upgrading to WebSocket");

        // 4. Upgrade to WebSocket.
        let ws_stream = tokio_tungstenite::accept_async(tcp_stream)
            .await
            .map_err(|e| Error::WebSocket(format!("WebSocket handshake failed: {e}")))?;

        // 5. Split into read/write halves.
        let (mut ws_write, mut ws_read) = ws_stream.split();

        self.ready = true;

        // Channels.
        let (read_tx, read_rx) = mpsc::channel::<Result<Value>>(256);
        let (write_tx, mut write_rx) = mpsc::channel::<Value>(256);
        let cancel = self.cancel.clone();

        // 6. Reader task: WS text frames -> parse JSON lines -> forward.
        let reader_cancel = cancel.clone();
        let reader_tx = read_tx;
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = reader_cancel.cancelled() => break,
                    frame = ws_read.next() => {
                        match frame {
                            Some(Ok(msg)) => {
                                let text = match msg {
                                    tungstenite::Message::Text(t) => t,
                                    tungstenite::Message::Close(_) => break,
                                    _ => continue,
                                };
                                // Each text frame may contain multiple newline-delimited JSON objects.
                                for line in text.split('\n') {
                                    let line = line.trim();
                                    if line.is_empty() {
                                        continue;
                                    }
                                    match serde_json::from_str::<Value>(line) {
                                        Ok(value) => {
                                            if reader_tx.send(Ok(value)).await.is_err() {
                                                return;
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!(line = %line, "failed to parse JSON from WS: {e}");
                                        }
                                    }
                                }
                            }
                            Some(Err(e)) => {
                                let _ = reader_tx
                                    .send(Err(Error::WebSocket(format!("WS read error: {e}"))))
                                    .await;
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        // 7. Writer task: channel -> serialize JSON -> send as WS text frame.
        let writer_cancel = cancel.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = writer_cancel.cancelled() => break,
                    msg = write_rx.recv() => {
                        match msg {
                            Some(value) => {
                                let mut data = match serde_json::to_string(&value) {
                                    Ok(s) => s,
                                    Err(e) => {
                                        tracing::error!("failed to serialize outgoing WS message: {e}");
                                        continue;
                                    }
                                };
                                data.push('\n');
                                if let Err(e) = ws_write
                                    .send(tungstenite::Message::Text(data.into()))
                                    .await
                                {
                                    tracing::error!("failed to send WS message: {e}");
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        // 8. Stderr reader task (identical to subprocess).
        if let Some(stderr) = stderr {
            let on_stderr = self.options.on_stderr.clone();
            let stderr_cancel = cancel;
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                loop {
                    tokio::select! {
                        _ = stderr_cancel.cancelled() => break,
                        line = lines.next_line() => {
                            match line {
                                Ok(Some(line)) => {
                                    if let Some(ref cb) = on_stderr {
                                        cb(line);
                                    } else {
                                        tracing::debug!(target: "claude_cli_stderr", "{}", line);
                                    }
                                }
                                Ok(None) | Err(_) => break,
                            }
                        }
                    }
                }
            });
        }

        let writer = TransportWriter::new(write_tx);
        Ok((read_rx, writer))
    }

    async fn close_impl(&mut self) -> Result<()> {
        self.ready = false;
        self.cancel.cancel();

        if let Some(ref mut child) = self.child {
            let _ = child.kill().await;
        }

        self.child = None;
        Ok(())
    }
}

impl Drop for WebSocketTransport {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
