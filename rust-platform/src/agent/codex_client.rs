//! Codex App-Server Client — manages the coding agent subprocess.
//!
//! Handles spawning the Codex app-server process, sending turn requests,
//! streaming JSON-line events, and graceful/forceful shutdown.
//!
//! SPEC reference: Section 10.1-10.3

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use chrono::{DateTime, Utc};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio_util::sync::CancellationToken;

use crate::config::service_config::CodexConfig;
use crate::models::CodexEventUpdate;
use crate::proxy::proxy_command;

/// Maximum line size for safe buffering (10 MB as per SPEC Section 10.1).
const MAX_LINE_SIZE: usize = 10 * 1024 * 1024;

fn build_initialize_request(id: u64) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "initialize",
        "params": {
            "clientInfo": {
                "name": "symphony-platform",
                "version": env!("CARGO_PKG_VERSION"),
            }
        },
        "id": id,
    })
}

fn build_thread_start_request(
    id: u64,
    workspace_path: &Path,
    approval_policy: Option<&serde_json::Value>,
    sandbox_policy: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut params = serde_json::json!({
        "cwd": workspace_path.to_string_lossy(),
    });

    if let Some(policy) = approval_policy {
        params["approvalPolicy"] = policy.clone();
    }
    if let Some(sandbox) = sandbox_policy {
        params["sandbox"] = sandbox_to_thread_value(sandbox);
    }

    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "thread/start",
        "params": params,
        "id": id,
    })
}

fn build_turn_start_request(
    id: u64,
    prompt: &str,
    workspace_path: &Path,
    thread_id: Option<&str>,
    sandbox_policy: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut params = serde_json::json!({
        "cwd": workspace_path.to_string_lossy(),
        "input": [{
            "type": "text",
            "text": prompt,
        }],
    });

    if let Some(tid) = thread_id {
        params["threadId"] = serde_json::Value::String(tid.to_string());
    }
    if let Some(sandbox) = sandbox_policy {
        params["sandboxPolicy"] = sandbox_to_turn_policy(sandbox);
    }

    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "turn/start",
        "params": params,
        "id": id,
    })
}

fn sandbox_to_thread_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => serde_json::Value::String(s.clone()),
        other => other.clone(),
    }
}

fn sandbox_to_turn_policy(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => serde_json::json!({
            "type": kebab_to_lower_camel(s),
        }),
        other => other.clone(),
    }
}

fn kebab_to_lower_camel(value: &str) -> String {
    let mut result = String::new();
    let mut uppercase_next = false;
    for ch in value.chars() {
        if ch == '-' || ch == '_' || ch == ' ' {
            uppercase_next = true;
        } else if uppercase_next {
            result.extend(ch.to_uppercase());
            uppercase_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

fn extract_thread_id_from_message(message: &serde_json::Value) -> Option<String> {
    message
        .pointer("/result/thread/id")
        .or_else(|| message.pointer("/result/threadId"))
        .or_else(|| message.pointer("/result/thread_id"))
        .or_else(|| message.pointer("/params/thread/id"))
        .or_else(|| message.pointer("/params/threadId"))
        .or_else(|| message.pointer("/params/thread_id"))
        .or_else(|| message.get("threadId"))
        .or_else(|| message.get("thread_id"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

fn extract_turn_id_from_message(message: &serde_json::Value) -> Option<String> {
    message
        .pointer("/params/turn/id")
        .or_else(|| message.pointer("/params/turnId"))
        .or_else(|| message.pointer("/params/turn_id"))
        .or_else(|| message.get("turnId"))
        .or_else(|| message.get("turn_id"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

fn extract_usage_from_message(
    message: &serde_json::Value,
) -> (Option<u64>, Option<u64>, Option<u64>) {
    let usage = message
        .get("usage")
        .or_else(|| message.pointer("/params/turn/usage"));

    usage
        .map(|u| {
            (
                u.get("input_tokens")
                    .or_else(|| u.get("inputTokens"))
                    .and_then(|v| v.as_u64()),
                u.get("output_tokens")
                    .or_else(|| u.get("outputTokens"))
                    .and_then(|v| v.as_u64()),
                u.get("total_tokens")
                    .or_else(|| u.get("totalTokens"))
                    .and_then(|v| v.as_u64()),
            )
        })
        .unwrap_or((None, None, None))
}

/// Errors from Codex client operations.
#[derive(Debug, Error)]
pub enum CodexError {
    #[error("codex command not found")]
    NotFound,

    #[error("invalid workspace cwd")]
    InvalidWorkspaceCwd,

    #[error("response timeout (read_timeout_ms exceeded)")]
    ResponseTimeout,

    #[error("turn timeout (turn_timeout_ms exceeded)")]
    TurnTimeout,

    #[error("codex process exited unexpectedly with code {code:?}")]
    ProcessExit { code: Option<i32> },

    #[error("response error: {detail}")]
    ResponseError { detail: String },

    #[error("turn failed: {reason}")]
    TurnFailed { reason: String },

    #[error("turn cancelled")]
    TurnCancelled,

    #[error("turn requires user input (not supported in high-trust mode)")]
    TurnInputRequired,

    #[error("malformed message from codex: {raw}")]
    MalformedMessage { raw: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::test_support::async_env_lock;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn turn_start_request_uses_app_server_schema() {
        let request = build_turn_start_request(
            7,
            "Fix issue ABC-1",
            Path::new("/tmp/work"),
            Some("thread-123"),
            Some(&json!("danger-full-access")),
        );

        assert_eq!(request["jsonrpc"], "2.0");
        assert_eq!(request["method"], "turn/start");
        assert_eq!(request["id"], 7);
        assert_eq!(request["params"]["threadId"], "thread-123");
        assert_eq!(
            request["params"]["input"],
            json!([
                {"type": "text", "text": "Fix issue ABC-1"}
            ])
        );
        assert!(request["params"].get("prompt").is_none());
        assert!(request["params"].get("thread_id").is_none());
        assert_eq!(
            request["params"]["sandboxPolicy"],
            json!({"type": "dangerFullAccess"})
        );
    }

    #[test]
    fn thread_start_request_uses_camel_case_policy_fields() {
        let request = build_thread_start_request(
            3,
            Path::new("/tmp/work"),
            Some(&json!("never")),
            Some(&json!("danger-full-access")),
        );

        assert_eq!(request["method"], "thread/start");
        assert_eq!(request["params"]["approvalPolicy"], "never");
        assert_eq!(request["params"]["sandbox"], "danger-full-access");
        assert!(request["params"].get("approval_policy").is_none());
    }

    #[tokio::test]
    async fn codex_app_server_command_exposes_proxy_env_to_shell_tools() {
        let mut env = async_env_lock().await;
        env.clear_proxy_env();
        let tmp = tempfile::tempdir().unwrap();
        let env_file = tmp.path().join("codex-tool-env.txt");
        env.set("SYMPHONY_PROXY_MODE", "manual");
        env.set("SYMPHONY_PROXY_VERSION", "22");
        env.set("SYMPHONY_PROXY_SOURCE", "system_config");
        env.set("HTTPS_PROXY", "http://proxy.example.com:8443");
        env.set("NO_PROXY", "localhost,127.0.0.1");

        let mut command = codex_app_server_command(tmp.path(), "env > \"$CODEX_TOOL_ENV_FILE\"");
        command.env("CODEX_TOOL_ENV_FILE", &env_file);
        let status = command.status().await.unwrap();
        let output = std::fs::read_to_string(&env_file).unwrap();

        assert!(status.success());
        assert!(output.contains("SYMPHONY_PROXY_MODE=manual"));
        assert!(output.contains("HTTPS_PROXY=http://proxy.example.com:8443"));
        assert!(output.contains("https_proxy=http://proxy.example.com:8443"));
        assert!(output.contains("NO_PROXY=localhost,127.0.0.1"));
        assert!(output.contains("no_proxy=localhost,127.0.0.1"));
    }

    #[test]
    fn extracts_thread_id_from_real_app_server_shapes() {
        let response = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "thread": { "id": "thread-from-result" }
            }
        });
        assert_eq!(
            extract_thread_id_from_message(&response).as_deref(),
            Some("thread-from-result")
        );

        let notification = json!({
            "jsonrpc": "2.0",
            "method": "thread/started",
            "params": {
                "threadId": "thread-from-notification"
            }
        });
        assert_eq!(
            extract_thread_id_from_message(&notification).as_deref(),
            Some("thread-from-notification")
        );
    }
}

/// Result of a completed turn.
#[derive(Debug, Clone)]
pub enum TurnResult {
    /// Turn completed successfully.
    Completed,
    /// Turn failed with a reason.
    Failed { reason: String },
    /// Turn requires user input (treated as failure in high-trust mode).
    InputRequired,
}

/// Codex App-Server Client — manages the child process lifecycle.
///
/// Communicates with the Codex app-server over stdio using JSON-RPC 2.0 protocol.
/// Each instance manages one subprocess that may serve multiple turns.
pub struct CodexClient {
    child: Child,
    stdin: tokio::io::BufWriter<tokio::process::ChildStdin>,
    stdout: BufReader<tokio::process::ChildStdout>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    pid: Option<String>,
    cancel_token: CancellationToken,
    turn_timeout_ms: u64,
    read_timeout_ms: u64,
    /// Workspace path for passing cwd in turn/start
    workspace_path: std::path::PathBuf,
    /// Approval policy from config
    approval_policy: Option<serde_json::Value>,
    /// Sandbox policy from config
    sandbox_policy: Option<serde_json::Value>,
    /// Incrementing JSON-RPC request ID
    next_rpc_id: u64,
    /// Whether the handshake (initialize + thread/start) has been completed
    handshake_done: bool,
}

impl CodexClient {
    /// Start the Codex app-server subprocess (SPEC Section 10.1).
    ///
    /// Spawns `bash -lc <codex.command>` with:
    /// - Working directory set to workspace_path
    /// - process_group(0) for clean process group kill
    /// - Piped stdin/stdout/stderr
    pub async fn start(
        workspace_path: &Path,
        codex_config: &CodexConfig,
        cancel_token: CancellationToken,
    ) -> Result<Self, CodexError> {
        // Validate workspace path exists
        if !workspace_path.is_dir() {
            return Err(CodexError::InvalidWorkspaceCwd);
        }

        tracing::info!(
            command = %codex_config.command,
            cwd = %workspace_path.display(),
            "starting codex app-server"
        );

        let mut command = codex_app_server_command(workspace_path, &codex_config.command);

        let mut child = command.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CodexError::NotFound
            } else {
                CodexError::Io(e)
            }
        })?;

        let pid = child.id().map(|p| p.to_string());

        let stdin = child.stdin.take().ok_or_else(|| {
            CodexError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "failed to capture stdin",
            ))
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            CodexError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "failed to capture stdout",
            ))
        })?;

        // Drain stderr in background to prevent pipe buffer deadlock
        if let Some(stderr) = child.stderr.take() {
            let pid_for_log = pid.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break,
                        Ok(_) => {
                            tracing::debug!(
                                pid = ?pid_for_log,
                                "codex stderr: {}",
                                line.trim_end()
                            );
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        tracing::info!(pid = ?pid, "codex app-server started");

        Ok(Self {
            child,
            stdin: tokio::io::BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            thread_id: None,
            turn_id: None,
            pid,
            cancel_token,
            turn_timeout_ms: codex_config.turn_timeout_ms,
            read_timeout_ms: codex_config.read_timeout_ms,
            workspace_path: workspace_path.to_path_buf(),
            approval_policy: codex_config.approval_policy.clone(),
            sandbox_policy: codex_config.turn_sandbox_policy.clone(),
            next_rpc_id: 1,
            handshake_done: false,
        })
    }

    /// Get the process ID of the Codex subprocess.
    pub fn pid(&self) -> Option<&str> {
        self.pid.as_deref()
    }

    /// Get the current thread ID.
    pub fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    /// Get the current turn ID.
    pub fn turn_id(&self) -> Option<&str> {
        self.turn_id.as_deref()
    }

    /// Execute one turn on the Codex app-server (SPEC Section 10.3).
    ///
    /// Sends a turn start request, then streams JSON-line events until the turn
    /// terminates. Supports cancellation via CancellationToken + tokio::select!.
    pub async fn run_turn(
        &mut self,
        prompt: &str,
        issue_id: &str,
        event_callback: &tokio::sync::mpsc::Sender<CodexEventUpdate>,
    ) -> Result<TurnResult, CodexError> {
        // Clone cancel_token to avoid borrow conflicts in select!
        let cancel = self.cancel_token.clone();

        tokio::select! {
            result = self.stream_turn_events(prompt, issue_id, event_callback) => result,
            _ = cancel.cancelled() => {
                tracing::info!("turn cancelled via cancellation token");
                self.kill().await;
                Err(CodexError::TurnCancelled)
            }
        }
    }

    /// Stream turn events from the Codex subprocess (internal).
    ///
    /// Sends the turn start request, then reads JSON-line events from stdout
    /// until a terminal event is detected or an error occurs.
    async fn stream_turn_events(
        &mut self,
        prompt: &str,
        issue_id: &str,
        event_callback: &tokio::sync::mpsc::Sender<CodexEventUpdate>,
    ) -> Result<TurnResult, CodexError> {
        // Send turn start request to stdin
        self.send_turn_start(prompt).await?;

        // Read JSON-line events from stdout
        let mut line_buf = String::with_capacity(4096);
        let turn_deadline =
            tokio::time::Instant::now() + Duration::from_millis(self.turn_timeout_ms);

        loop {
            line_buf.clear();

            // Read with turn timeout
            let read_result =
                tokio::time::timeout_at(turn_deadline, self.stdout.read_line(&mut line_buf)).await;

            let bytes_read = match read_result {
                Err(_) => {
                    // Turn timeout exceeded
                    tracing::error!(issue_id, "turn timeout exceeded");
                    return Err(CodexError::TurnTimeout);
                }
                Ok(Err(e)) => {
                    // I/O error reading from stdout
                    tracing::error!(issue_id, error = %e, "error reading from codex stdout");
                    return Err(CodexError::ProcessExit {
                        code: self.child.try_wait().ok().flatten().and_then(|s| s.code()),
                    });
                }
                Ok(Ok(n)) => n,
            };

            if bytes_read == 0 {
                // EOF — process exited
                let code = self.child.try_wait().ok().flatten().and_then(|s| s.code());
                tracing::warn!(issue_id, exit_code = ?code, "codex process exited during turn");
                return Err(CodexError::ProcessExit { code });
            }

            // Safety: reject excessively long lines
            if line_buf.len() > MAX_LINE_SIZE {
                tracing::warn!(
                    issue_id,
                    len = line_buf.len(),
                    "dropping oversized line from codex"
                );
                continue;
            }

            let trimmed = line_buf.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse JSON-line event
            match serde_json::from_str::<serde_json::Value>(trimmed) {
                Ok(event) => {
                    // Handle approval requests (SPEC 10.5: MUST NOT stall)
                    if self.is_approval_request(&event) {
                        if let Err(e) = self.handle_approval_request(&event).await {
                            tracing::warn!(issue_id, error = %e, "failed to send approval response");
                        }
                        continue;
                    }

                    // Handle dynamic tool calls
                    if self.is_tool_call(&event) {
                        if let Err(e) = self.handle_tool_call(&event).await {
                            tracing::warn!(issue_id, error = %e, "failed to send tool result");
                        }
                        continue;
                    }

                    let update = self.extract_codex_event(&event);

                    // Check for turn terminal events
                    if let Some(result) = self.check_turn_terminal(&event) {
                        // Send final event update
                        let _ = event_callback.send(update).await;
                        return result;
                    }

                    // Non-terminal event: send to orchestrator
                    if event_callback.send(update).await.is_err() {
                        // Channel closed — orchestrator shut down
                        tracing::warn!(issue_id, "event channel closed, killing codex");
                        self.kill().await;
                        return Err(CodexError::TurnCancelled);
                    }
                }
                Err(_) => {
                    tracing::debug!(issue_id, raw = trimmed, "malformed JSON-line from codex");
                }
            }
        }
    }

    /// Send a turn start request to the Codex subprocess stdin.
    /// Performs JSON-RPC 2.0 handshake on first call (initialize → thread/start → turn/start).
    /// On subsequent calls, reuses thread_id (turn/start only).
    async fn send_turn_start(&mut self, prompt: &str) -> Result<(), CodexError> {
        if !self.handshake_done {
            self.perform_handshake().await?;
        }

        let id = self.next_rpc_id();
        let request = build_turn_start_request(
            id,
            prompt,
            &self.workspace_path,
            self.thread_id.as_deref(),
            self.sandbox_policy.as_ref(),
        );

        self.send_json_rpc(&request).await
    }

    /// Perform the JSON-RPC 2.0 handshake: initialize → thread/start.
    async fn perform_handshake(&mut self) -> Result<(), CodexError> {
        let handshake_deadline =
            tokio::time::Instant::now() + Duration::from_millis(self.read_timeout_ms);

        // Step 1: Send initialize request
        let init_id = self.next_rpc_id();
        let init_request = build_initialize_request(init_id);

        tracing::info!("sending initialize request (id={})", init_id);
        self.send_json_rpc(&init_request).await?;
        tracing::info!("initialize request sent, waiting for response...");

        // Read initialize response (with read_timeout)
        let _init_response = self
            .read_json_rpc_response(handshake_deadline, Some(init_id))
            .await?;

        tracing::info!("initialize response received");

        // Step 2: Send thread/start request
        let thread_id = self.next_rpc_id();
        let thread_request = build_thread_start_request(
            thread_id,
            &self.workspace_path,
            self.approval_policy.as_ref(),
            self.sandbox_policy.as_ref(),
        );

        self.send_json_rpc(&thread_request).await?;

        tracing::info!(
            "thread/start request sent (id={}), waiting for response...",
            thread_id
        );

        // Read thread/start response to get thread_id
        let thread_response = self
            .read_json_rpc_response(handshake_deadline, Some(thread_id))
            .await?;

        if let Some(tid) = extract_thread_id_from_message(&thread_response) {
            self.thread_id = Some(tid);
        }

        self.handshake_done = true;
        tracing::info!(thread_id = ?self.thread_id, "JSON-RPC 2.0 handshake completed");
        Ok(())
    }

    /// Send a JSON-RPC message to the Codex subprocess stdin.
    async fn send_json_rpc(&mut self, message: &serde_json::Value) -> Result<(), CodexError> {
        let mut line = serde_json::to_string(message)?;
        line.push('\n');

        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(CodexError::Io)?;
        self.stdin.flush().await.map_err(CodexError::Io)?;

        Ok(())
    }

    /// Read a JSON-RPC response with a deadline.
    async fn read_json_rpc_response(
        &mut self,
        deadline: tokio::time::Instant,
        expected_id: Option<u64>,
    ) -> Result<serde_json::Value, CodexError> {
        let mut line_buf = String::with_capacity(4096);

        loop {
            line_buf.clear();
            let read_result =
                tokio::time::timeout_at(deadline, self.stdout.read_line(&mut line_buf)).await;

            let bytes_read = match read_result {
                Err(_) => {
                    tracing::error!(
                        expected_id = ?expected_id,
                        "read_json_rpc_response timed out waiting for response"
                    );
                    return Err(CodexError::ResponseTimeout);
                }
                Ok(Err(e)) => return Err(CodexError::Io(e)),
                Ok(Ok(n)) => n,
            };

            if bytes_read == 0 {
                let code = self.child.try_wait().ok().flatten().and_then(|s| s.code());
                return Err(CodexError::ProcessExit { code });
            }

            let trimmed = line_buf.trim();
            if trimmed.is_empty() {
                continue;
            }

            match serde_json::from_str::<serde_json::Value>(trimmed) {
                Ok(value) => {
                    if let Some(tid) = extract_thread_id_from_message(&value) {
                        self.thread_id = Some(tid);
                    }
                    if let Some(tid) = extract_turn_id_from_message(&value) {
                        self.turn_id = Some(tid);
                    }

                    // Check for JSON-RPC error response
                    if let Some(error) = value.get("error") {
                        let detail = error
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown error")
                            .to_string();
                        return Err(CodexError::ResponseError { detail });
                    }

                    if let Some(expected) = expected_id {
                        if value.get("id").and_then(|v| v.as_u64()) != Some(expected) {
                            let method = value
                                .get("method")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            tracing::debug!(
                                expected_id,
                                got_id = ?value.get("id"),
                                method,
                                "skipping non-matching message during handshake"
                            );
                            continue;
                        }
                    }

                    return Ok(value);
                }
                Err(_) => {
                    // Skip non-JSON lines during handshake (e.g. startup logs)
                    continue;
                }
            }
        }
    }

    /// Get the next JSON-RPC request ID.
    fn next_rpc_id(&mut self) -> u64 {
        let id = self.next_rpc_id;
        self.next_rpc_id += 1;
        id
    }

    /// Handle an approval request event by auto-approving (SPEC 10.5).
    async fn handle_approval_request(
        &mut self,
        event: &serde_json::Value,
    ) -> Result<(), CodexError> {
        let request_id = event
            .get("id")
            .or_else(|| event.get("request_id"))
            .or_else(|| event.pointer("/data/request_id"))
            .or_else(|| event.pointer("/params/requestId"))
            .or_else(|| event.pointer("/params/request_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        tracing::info!(request_id, "auto-approving codex approval request");

        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "approval/resolve",
            "params": {
                "id": request_id,
                "approved": true,
            },
            "id": self.next_rpc_id(),
        });

        self.send_json_rpc(&response).await
    }

    /// Handle a dynamic tool call event.
    async fn handle_tool_call(&mut self, event: &serde_json::Value) -> Result<(), CodexError> {
        let call_id = event
            .get("call_id")
            .or_else(|| event.pointer("/data/call_id"))
            .or_else(|| event.pointer("/params/callId"))
            .or_else(|| event.pointer("/params/call_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        tracing::info!(
            call_id,
            "received tool call (not yet implemented, returning error)"
        );

        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tool/result",
            "params": {
                "call_id": call_id,
                "error": "tool not available",
            },
            "id": self.next_rpc_id(),
        });

        self.send_json_rpc(&response).await
    }

    /// Extract a structured event update from a raw JSON event.
    fn extract_codex_event(&mut self, event: &serde_json::Value) -> CodexEventUpdate {
        let event_type = event
            .get("type")
            .or_else(|| event.get("event"))
            .or_else(|| event.get("method"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let timestamp = event
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(Utc::now);

        // Extract thread_id and turn_id if present
        if let Some(tid) = extract_thread_id_from_message(event) {
            self.thread_id = Some(tid);
        }
        if let Some(tid) = extract_turn_id_from_message(event) {
            self.turn_id = Some(tid);
        }

        let session_id = match (&self.thread_id, &self.turn_id) {
            (Some(t), Some(u)) => Some(format!("{}-{}", t, u)),
            _ => None,
        };

        // Extract token usage
        let (input_tokens, output_tokens, total_tokens) = extract_usage_from_message(event);

        let rate_limits = event.get("rate_limits").cloned();

        let message = event
            .get("message")
            .or_else(|| event.get("content"))
            .and_then(|v| v.as_str())
            .map(|s| {
                // Truncate long messages for observability
                if s.len() > 200 {
                    format!("{}...", &s[..200])
                } else {
                    s.to_string()
                }
            });

        CodexEventUpdate {
            event_type: Some(event_type),
            timestamp: Some(timestamp),
            pid: self.pid.clone(),
            session_id,
            thread_id: self.thread_id.clone(),
            turn_id: self.turn_id.clone(),
            input_tokens,
            output_tokens,
            total_tokens,
            rate_limits,
            message,
        }
    }

    /// Check if a JSON event represents a turn terminal condition.
    ///
    /// Returns Some(result) if the turn has ended, None if it should continue.
    fn check_turn_terminal(
        &self,
        event: &serde_json::Value,
    ) -> Option<Result<TurnResult, CodexError>> {
        let event_type = event
            .get("type")
            .or_else(|| event.get("event"))
            .or_else(|| event.get("method"))
            .and_then(|v| v.as_str())?;

        match event_type {
            // TODO(Phase 3): 处理 approval 请求事件 (commandExecution/fileChange)
            //   Phase 3 实现自动审批：根据 approval_policy 配置自动回复 approve/reject。
            // TODO(Phase 6): 操作员介入模式 — 当 approval_policy.on_reject_match == "ask" 时，
            //   暂停 turn 并通过 HTTP API 暴露 pending approval，等待操作员决策。
            //   参见 docs/migration-gap-analysis.md Phase 6。

            // Turn completed successfully
            "turn.completed" | "turn_completed" | "turn/completed" => {
                Some(Ok(TurnResult::Completed))
            }

            // Turn failed
            "turn.failed" | "turn_failed" | "turn/failed" => {
                let reason = event
                    .get("error")
                    .or_else(|| event.get("reason"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                Some(Ok(TurnResult::Failed { reason }))
            }

            // Turn cancelled
            "turn.cancelled" | "turn_cancelled" | "turn/cancelled" => {
                Some(Err(CodexError::TurnCancelled))
            }

            // Turn requires user input (high-trust mode: treat as failure)
            // TODO(Phase 6): 操作员介入模式 — 当 user_input_policy == "ask" 时，
            //   暂停 turn 并通过 HTTP API 暴露 pending question，等待操作员回答。
            //   当前策略：立即 fail turn → 触发 retry。
            //   参见 docs/migration-gap-analysis.md Phase 6。
            "turn.input_required" | "turn_input_required" | "input_required" => {
                Some(Err(CodexError::TurnInputRequired))
            }

            // Turn ended with error
            "turn.error" | "turn_ended_with_error" | "turn/error" => {
                let detail = event
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error")
                    .to_string();
                Some(Err(CodexError::TurnFailed { reason: detail }))
            }

            _ => None,
        }
    }

    /// Check if an event is an approval request.
    fn is_approval_request(&self, event: &serde_json::Value) -> bool {
        let event_type = event
            .get("type")
            .or_else(|| event.get("event"))
            .or_else(|| event.get("method"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        matches!(
            event_type,
            "approval/request" | "approval_request" | "commandExecution" | "fileChange"
        )
    }

    /// Check if an event is a dynamic tool call.
    fn is_tool_call(&self, event: &serde_json::Value) -> bool {
        let event_type = event
            .get("type")
            .or_else(|| event.get("event"))
            .or_else(|| event.get("method"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        matches!(event_type, "item/tool/call" | "tool/call" | "tool_call")
    }

    /// Gracefully stop the Codex subprocess (SPEC Section 10.3).
    ///
    /// Attempts a graceful shutdown with a 5-second timeout,
    /// then kills the entire process group if still running.
    pub async fn stop(&mut self) {
        tracing::info!(pid = ?self.pid, "stopping codex app-server");

        // Try to send a stop signal via stdin (best-effort)
        let stop_msg = serde_json::json!({"type": "stop"});
        if let Ok(line) = serde_json::to_string(&stop_msg) {
            let msg = format!("{}\n", line);
            let _ = self.stdin.write_all(msg.as_bytes()).await;
            let _ = self.stdin.flush().await;
        }

        // Wait for graceful exit with timeout
        match tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await {
            Ok(Ok(status)) => {
                tracing::info!(pid = ?self.pid, exit_code = ?status.code(), "codex stopped gracefully");
            }
            _ => {
                // Timeout or error — force kill
                tracing::warn!(pid = ?self.pid, "codex did not stop gracefully, killing process group");
                self.kill().await;
            }
        }
    }

    /// Force-kill the Codex subprocess and its entire process group.
    ///
    /// Uses SIGKILL to the process group (negative PID) via libc::kill
    /// to ensure all child processes are terminated.
    pub async fn kill(&mut self) {
        if let Some(pid) = self.child.id() {
            tracing::info!(pid, "sending SIGKILL to codex process group");
            // Kill the entire process group (negative PID)
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        // Also kill via tokio's child handle as a fallback
        let _ = self.child.kill().await;
        // Reap the zombie
        let _ = self.child.wait().await;
    }
}

fn codex_app_server_command(workspace_path: &Path, command_line: &str) -> tokio::process::Command {
    let mut command = proxy_command("bash");
    command
        .args(["-lc", command_line])
        .current_dir(workspace_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Create new process group for clean kill propagation
        .process_group(0);
    command
}

impl Drop for CodexClient {
    fn drop(&mut self) {
        // Best-effort cleanup: try to kill the process group on drop
        if let Some(pid) = self.child.id() {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}
