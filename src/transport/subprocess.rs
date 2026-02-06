use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::types::options::{ClaudeAgentOptions, StderrCallback};
use crate::types::permissions::PermissionMode;

use super::{Transport, TransportWriter};

/// Transport implementation that communicates with the Claude CLI via subprocess.
pub struct SubprocessTransport {
    cli_path: PathBuf,
    options: BuildOptions,
    child: Option<Child>,
    cancel: CancellationToken,
    ready: bool,
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

impl SubprocessTransport {
    pub fn new(cli_path: PathBuf, options: &ClaudeAgentOptions) -> Self {
        Self {
            cli_path,
            options: BuildOptions::from(options),
            child: None,
            cancel: CancellationToken::new(),
            ready: false,
        }
    }

    /// Build the CLI command with all flags.
    fn build_command(&self) -> Command {
        let mut cmd = Command::new(&self.cli_path);

        cmd.args(["--output-format", "stream-json"]);
        cmd.args(["--input-format", "stream-json"]);
        cmd.arg("--verbose");

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

        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        cmd
    }
}

impl Transport for SubprocessTransport {
    fn connect(&mut self) -> Pin<Box<dyn Future<Output = Result<(mpsc::Receiver<Result<Value>>, TransportWriter)>> + Send + '_>> {
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

impl SubprocessTransport {
    async fn connect_impl(&mut self) -> Result<(mpsc::Receiver<Result<Value>>, TransportWriter)> {
        if self.ready {
            return Err(Error::AlreadyConnected);
        }

        let mut cmd = self.build_command();
        let mut child = cmd
            .spawn()
            .map_err(|e| Error::CliConnection(format!("failed to spawn CLI: {e}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::CliConnection("no stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| Error::CliConnection("no stderr".into()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::CliConnection("no stdin".into()))?;

        self.child = Some(child);
        self.ready = true;

        // Incoming message channel (stdout -> reader).
        let (read_tx, read_rx) = mpsc::channel::<Result<Value>>(256);

        // Outgoing message channel (writer -> stdin).
        let (write_tx, mut write_rx) = mpsc::channel::<Value>(256);

        let cancel = self.cancel.clone();

        // Stdout reader task.
        let stdout_tx = read_tx;
        let stdout_cancel = cancel.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            loop {
                tokio::select! {
                    _ = stdout_cancel.cancelled() => break,
                    line = lines.next_line() => {
                        match line {
                            Ok(Some(line)) => {
                                let line = line.trim().to_string();
                                if line.is_empty() {
                                    continue;
                                }
                                match serde_json::from_str::<Value>(&line) {
                                    Ok(value) => {
                                        if stdout_tx.send(Ok(value)).await.is_err() {
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(line = %line, "failed to parse JSON from CLI: {e}");
                                    }
                                }
                            }
                            Ok(None) => break,
                            Err(e) => {
                                let _ = stdout_tx.send(Err(Error::Io(e))).await;
                                break;
                            }
                        }
                    }
                }
            }
        });

        // Stdin writer task: reads from write channel, serializes to stdin.
        let write_cancel = cancel.clone();
        tokio::spawn(async move {
            let mut stdin = stdin;
            loop {
                tokio::select! {
                    _ = write_cancel.cancelled() => break,
                    msg = write_rx.recv() => {
                        match msg {
                            Some(value) => {
                                let mut data = match serde_json::to_string(&value) {
                                    Ok(s) => s,
                                    Err(e) => {
                                        tracing::error!("failed to serialize outgoing message: {e}");
                                        continue;
                                    }
                                };
                                data.push('\n');

                                if let Err(e) = stdin.write_all(data.as_bytes()).await {
                                    tracing::error!("failed to write to stdin: {e}");
                                    break;
                                }
                                if let Err(e) = stdin.flush().await {
                                    tracing::error!("failed to flush stdin: {e}");
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        // Stderr reader task.
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

impl Drop for SubprocessTransport {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
