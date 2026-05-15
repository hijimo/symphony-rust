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
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

use crate::config::service_config::CodexConfig;

/// Maximum line size for safe buffering (10 MB as per SPEC Section 10.1).
const MAX_LINE_SIZE: usize = 10 * 1024 * 1024;

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

/// Token usage information extracted from Codex events.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Structured event update extracted from Codex JSON-line events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexEventUpdate {
    pub event: String,
    pub timestamp: DateTime<Utc>,
    pub pid: Option<String>,
    pub session_id: Option<String>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub usage: Option<TokenUsage>,
    pub rate_limits: Option<serde_json::Value>,
    pub message: Option<String>,
}

/// Codex App-Server Client — manages the child process lifecycle.
///
/// Communicates with the Codex app-server over stdio using JSON-line protocol.
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
    #[allow(dead_code)]
    read_timeout_ms: u64,
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

        let mut child = Command::new("bash")
            .args(["-lc", &codex_config.command])
            .current_dir(workspace_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Create new process group for clean kill propagation
            .process_group(0)
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    CodexError::NotFound
                } else {
                    CodexError::Io(e)
                }
            })?;

        let pid = child.id().map(|p| p.to_string());

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CodexError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "failed to capture stdin",
            )))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CodexError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "failed to capture stdout",
            )))?;

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
        let turn_deadline = tokio::time::Instant::now()
            + Duration::from_millis(self.turn_timeout_ms);

        loop {
            line_buf.clear();

            // Read with turn timeout
            let read_result = tokio::time::timeout_at(
                turn_deadline,
                self.stdout.read_line(&mut line_buf),
            )
            .await;

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
                tracing::warn!(issue_id, len = line_buf.len(), "dropping oversized line from codex");
                continue;
            }

            let trimmed = line_buf.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse JSON-line event
            match serde_json::from_str::<serde_json::Value>(trimmed) {
                Ok(event) => {
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
    async fn send_turn_start(&mut self, prompt: &str) -> Result<(), CodexError> {
        let request = serde_json::json!({
            "type": "turn.start",
            "prompt": prompt,
        });

        let mut line = serde_json::to_string(&request)?;
        line.push('\n');

        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| CodexError::Io(e))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| CodexError::Io(e))?;

        Ok(())
    }

    /// Extract a structured event update from a raw JSON event.
    fn extract_codex_event(&mut self, event: &serde_json::Value) -> CodexEventUpdate {
        let event_type = event
            .get("type")
            .or_else(|| event.get("event"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let timestamp = event
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(Utc::now);

        // Extract thread_id and turn_id if present
        if let Some(tid) = event.get("thread_id").and_then(|v| v.as_str()) {
            self.thread_id = Some(tid.to_string());
        }
        if let Some(tid) = event.get("turn_id").and_then(|v| v.as_str()) {
            self.turn_id = Some(tid.to_string());
        }

        let session_id = match (&self.thread_id, &self.turn_id) {
            (Some(t), Some(u)) => Some(format!("{}-{}", t, u)),
            _ => None,
        };

        // Extract token usage
        let usage = event.get("usage").and_then(|u| {
            Some(TokenUsage {
                input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            })
        });

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
            event: event_type,
            timestamp,
            pid: self.pid.clone(),
            session_id,
            thread_id: self.thread_id.clone(),
            turn_id: self.turn_id.clone(),
            usage,
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
            .and_then(|v| v.as_str())?;

        match event_type {
            // Turn completed successfully
            "turn.completed" | "turn_completed" => Some(Ok(TurnResult::Completed)),

            // Turn failed
            "turn.failed" | "turn_failed" => {
                let reason = event
                    .get("error")
                    .or_else(|| event.get("reason"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                Some(Ok(TurnResult::Failed { reason }))
            }

            // Turn cancelled
            "turn.cancelled" | "turn_cancelled" => {
                Some(Err(CodexError::TurnCancelled))
            }

            // Turn requires user input (high-trust mode: treat as failure)
            "turn.input_required" | "turn_input_required" | "input_required" => {
                Some(Err(CodexError::TurnInputRequired))
            }

            // Turn ended with error
            "turn.error" | "turn_ended_with_error" => {
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
