#![allow(
    unused_imports,
    unused_variables,
    dead_code,
    clippy::bind_instead_of_map,
    clippy::derivable_impls,
    clippy::manual_range_contains,
    clippy::needless_borrows_for_generic_args,
    clippy::ptr_arg,
    clippy::duplicated_attributes,
    clippy::approx_constant,
    clippy::bool_assert_comparison,
    clippy::len_zero,
    clippy::let_and_return
)]

//! FakeCodexProcess — simulates the codex app-server subprocess for E2E tests.
//!
//! Instead of spawning a real codex binary, this module provides a configurable
//! fake that emits JSON-line events on stdout and exits with a specified code.
//!
//! Configuration is done via `CodexBehavior` which controls:
//! - Events to emit (with optional delays between them)
//! - Turn duration (simulated processing time)
//! - Exit code
//! - Stall behavior (for stall detection tests)
//!
//! Extended capabilities:
//! - Multi-turn conversations (reuses thread_id)
//! - Approval request simulation
//! - Tool call simulation
//! - JSON-RPC 2.0 protocol support (initialize, thread/start, turn/start)

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

/// A single event emitted by the fake codex process on stdout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexEvent {
    /// Event type (e.g., "turn.start", "turn.end", "message", "error")
    #[serde(rename = "type")]
    pub event_type: String,
    /// Optional payload data
    #[serde(default)]
    pub data: serde_json::Value,
    /// Timestamp (auto-generated if not set)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

impl CodexEvent {
    pub fn turn_start() -> Self {
        Self {
            event_type: "turn.start".to_string(),
            data: serde_json::json!({}),
            timestamp: None,
        }
    }

    pub fn turn_end() -> Self {
        Self {
            event_type: "turn.end".to_string(),
            data: serde_json::json!({}),
            timestamp: None,
        }
    }

    pub fn turn_completed() -> Self {
        Self {
            event_type: "turn.completed".to_string(),
            data: serde_json::json!({"status": "completed"}),
            timestamp: None,
        }
    }

    pub fn turn_failed(reason: &str) -> Self {
        Self {
            event_type: "turn.failed".to_string(),
            data: serde_json::json!({"error": reason}),
            timestamp: None,
        }
    }

    pub fn message(content: &str) -> Self {
        Self {
            event_type: "message".to_string(),
            data: serde_json::json!({"content": content}),
            timestamp: None,
        }
    }

    pub fn error(message: &str) -> Self {
        Self {
            event_type: "error".to_string(),
            data: serde_json::json!({"message": message}),
            timestamp: None,
        }
    }

    pub fn token_usage(input: u64, output: u64) -> Self {
        Self {
            event_type: "token.usage".to_string(),
            data: serde_json::json!({"input_tokens": input, "output_tokens": output}),
            timestamp: None,
        }
    }

    pub fn session_started(thread_id: &str, turn_id: &str) -> Self {
        Self {
            event_type: "session_started".to_string(),
            data: serde_json::json!({
                "thread_id": thread_id,
                "turn_id": turn_id,
            }),
            timestamp: None,
        }
    }

    pub fn approval_request(tool_name: &str, args: serde_json::Value) -> Self {
        Self {
            event_type: "approval_request".to_string(),
            data: serde_json::json!({
                "tool": tool_name,
                "arguments": args,
                "request_id": "approval-001",
            }),
            timestamp: None,
        }
    }

    pub fn tool_call(tool_name: &str, args: serde_json::Value) -> Self {
        Self {
            event_type: "tool_call".to_string(),
            data: serde_json::json!({
                "tool": tool_name,
                "arguments": args,
                "call_id": "call-001",
            }),
            timestamp: None,
        }
    }

    pub fn tool_result(call_id: &str, result: serde_json::Value) -> Self {
        Self {
            event_type: "tool_result".to_string(),
            data: serde_json::json!({
                "call_id": call_id,
                "result": result,
            }),
            timestamp: None,
        }
    }

    pub fn input_required(prompt: &str) -> Self {
        Self {
            event_type: "turn.input_required".to_string(),
            data: serde_json::json!({"prompt": prompt}),
            timestamp: None,
        }
    }
}

/// Scenario for multi-turn behavior.
#[derive(Debug, Clone)]
pub struct TurnScenario {
    /// Events to emit during this turn
    pub events: Vec<CodexEvent>,
    /// Delay between events in this turn
    pub event_delay: Duration,
    /// Whether this turn completes successfully
    pub completes: bool,
    /// If not completing, the failure reason
    pub failure_reason: Option<String>,
}

impl TurnScenario {
    /// A successful turn with standard events.
    pub fn success() -> Self {
        Self {
            events: vec![
                CodexEvent::turn_start(),
                CodexEvent::message("Working on the issue..."),
                CodexEvent::token_usage(500, 200),
                CodexEvent::turn_completed(),
            ],
            event_delay: Duration::from_millis(5),
            completes: true,
            failure_reason: None,
        }
    }

    /// A turn that fails with the given reason.
    pub fn failure(reason: &str) -> Self {
        Self {
            events: vec![
                CodexEvent::turn_start(),
                CodexEvent::error(reason),
                CodexEvent::turn_failed(reason),
            ],
            event_delay: Duration::from_millis(5),
            completes: false,
            failure_reason: Some(reason.to_string()),
        }
    }

    /// A turn that requires approval.
    pub fn needs_approval(tool_name: &str) -> Self {
        Self {
            events: vec![
                CodexEvent::turn_start(),
                CodexEvent::message("I need to run a command..."),
                CodexEvent::approval_request(
                    tool_name,
                    serde_json::json!({"command": "rm -rf /tmp/test"}),
                ),
            ],
            event_delay: Duration::from_millis(5),
            completes: false,
            failure_reason: None,
        }
    }

    /// A turn that makes a tool call.
    pub fn with_tool_call(tool_name: &str, result: serde_json::Value) -> Self {
        Self {
            events: vec![
                CodexEvent::turn_start(),
                CodexEvent::tool_call(tool_name, serde_json::json!({"path": "/tmp/test.rs"})),
                CodexEvent::tool_result("call-001", result),
                CodexEvent::message("Tool call completed."),
                CodexEvent::turn_completed(),
            ],
            event_delay: Duration::from_millis(5),
            completes: true,
            failure_reason: None,
        }
    }

    /// A turn that requires user input.
    pub fn needs_input(prompt: &str) -> Self {
        Self {
            events: vec![CodexEvent::turn_start(), CodexEvent::input_required(prompt)],
            event_delay: Duration::from_millis(5),
            completes: false,
            failure_reason: None,
        }
    }
}

/// Configures the behavior of a fake codex process.
#[derive(Debug, Clone)]
pub struct CodexBehavior {
    /// Events to emit on stdout (JSON-line format)
    pub events: Vec<CodexEvent>,
    /// Delay between each event emission
    pub event_delay: Duration,
    /// Total simulated turn duration (sleep before exit)
    pub turn_duration: Duration,
    /// Exit code to return
    pub exit_code: i32,
    /// If true, the process will stall (never exit, stop emitting events)
    pub stall: bool,
    /// Delay before stalling (only relevant if stall=true)
    pub stall_after: Duration,
    /// Multi-turn scenarios (overrides events if non-empty)
    pub turn_scenarios: Vec<TurnScenario>,
    /// Maximum number of turns before exiting
    pub max_turns: Option<u32>,
}

impl Default for CodexBehavior {
    fn default() -> Self {
        Self {
            events: vec![
                CodexEvent::turn_start(),
                CodexEvent::message("Working on the issue..."),
                CodexEvent::token_usage(500, 200),
                CodexEvent::turn_end(),
            ],
            event_delay: Duration::from_millis(10),
            turn_duration: Duration::from_millis(50),
            exit_code: 0,
            stall: false,
            stall_after: Duration::from_millis(100),
            turn_scenarios: Vec::new(),
            max_turns: None,
        }
    }
}

impl CodexBehavior {
    /// Create a behavior that completes successfully with minimal events.
    pub fn success() -> Self {
        Self::default()
    }

    /// Create a behavior that exits with a non-zero code (agent error).
    pub fn failure(exit_code: i32) -> Self {
        Self {
            events: vec![
                CodexEvent::turn_start(),
                CodexEvent::error("Something went wrong"),
                CodexEvent::turn_end(),
            ],
            exit_code,
            ..Self::default()
        }
    }

    /// Create a behavior that stalls (for stall detection tests).
    pub fn stalling(stall_after: Duration) -> Self {
        Self {
            events: vec![CodexEvent::turn_start()],
            stall: true,
            stall_after,
            ..Self::default()
        }
    }

    /// Create a behavior with custom turn duration.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.turn_duration = duration;
        self
    }

    /// Create a behavior with custom events.
    pub fn with_events(mut self, events: Vec<CodexEvent>) -> Self {
        self.events = events;
        self
    }

    /// Create a multi-turn behavior from a sequence of turn scenarios.
    pub fn multi_turn(scenarios: Vec<TurnScenario>) -> Self {
        Self {
            events: Vec::new(), // Ignored when turn_scenarios is non-empty
            turn_scenarios: scenarios,
            ..Self::default()
        }
    }

    /// Create a behavior that completes in exactly N turns.
    pub fn n_turns(n: u32) -> Self {
        let scenarios: Vec<TurnScenario> = (0..n).map(|_| TurnScenario::success()).collect();
        Self::multi_turn(scenarios)
    }

    /// Create a behavior that hits max_turns limit.
    pub fn hits_max_turns(max_turns: u32) -> Self {
        // Create more scenarios than max_turns allows
        let scenarios: Vec<TurnScenario> = (0..max_turns + 5)
            .map(|_| TurnScenario::success())
            .collect();
        Self {
            max_turns: Some(max_turns),
            ..Self::multi_turn(scenarios)
        }
    }

    /// Create a behavior with an approval request on a specific turn.
    pub fn with_approval_on_turn(total_turns: u32, approval_turn: u32) -> Self {
        let scenarios: Vec<TurnScenario> = (0..total_turns)
            .map(|i| {
                if i == approval_turn - 1 {
                    TurnScenario::needs_approval("commandExecution")
                } else {
                    TurnScenario::success()
                }
            })
            .collect();
        Self::multi_turn(scenarios)
    }

    /// Create a behavior with a tool call on a specific turn.
    pub fn with_tool_call_on_turn(total_turns: u32, tool_turn: u32) -> Self {
        let scenarios: Vec<TurnScenario> = (0..total_turns)
            .map(|i| {
                if i == tool_turn - 1 {
                    TurnScenario::with_tool_call(
                        "read_file",
                        serde_json::json!({"content": "fn main() {}"}),
                    )
                } else {
                    TurnScenario::success()
                }
            })
            .collect();
        Self::multi_turn(scenarios)
    }

    /// Create a behavior that fails on a specific turn then succeeds.
    pub fn fail_then_succeed(fail_turn: u32, total_turns: u32) -> Self {
        let scenarios: Vec<TurnScenario> = (0..total_turns)
            .map(|i| {
                if i == fail_turn - 1 {
                    TurnScenario::failure("transient error")
                } else {
                    TurnScenario::success()
                }
            })
            .collect();
        Self::multi_turn(scenarios)
    }

    /// Create a slow behavior for timeout testing.
    pub fn slow(delay_per_event: Duration) -> Self {
        Self {
            events: vec![
                CodexEvent::turn_start(),
                CodexEvent::message("Processing slowly..."),
                CodexEvent::token_usage(100, 50),
                CodexEvent::turn_end(),
            ],
            event_delay: delay_per_event,
            turn_duration: Duration::from_millis(10),
            ..Self::default()
        }
    }
}

/// A fake codex process that runs in-process (no actual subprocess).
///
/// This simulates the codex app-server by:
/// 1. Emitting configured events as JSON lines
/// 2. Sleeping for the configured turn duration
/// 3. Exiting with the configured exit code
///
/// For tests that need a real subprocess, use `spawn_fake_codex_script()`.
pub struct FakeCodexProcess {
    behavior: CodexBehavior,
    /// Channel to receive events as they are emitted (for test assertions)
    event_tx: Option<mpsc::UnboundedSender<CodexEvent>>,
}

impl FakeCodexProcess {
    pub fn new(behavior: CodexBehavior) -> Self {
        Self {
            behavior,
            event_tx: None,
        }
    }

    /// Attach an event observer channel.
    pub fn with_event_observer(mut self, tx: mpsc::UnboundedSender<CodexEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Run the fake codex process, emitting events and returning the exit code.
    ///
    /// This is meant to be called from within a spawned task that simulates
    /// the agent runner reading from the codex subprocess.
    pub async fn run(&self) -> (Vec<String>, i32) {
        let mut output_lines = Vec::new();

        // If multi-turn scenarios are configured, use those
        if !self.behavior.turn_scenarios.is_empty() {
            return self.run_multi_turn().await;
        }

        for event in &self.behavior.events {
            let line = serde_json::to_string(event).unwrap();
            output_lines.push(line);

            if let Some(tx) = &self.event_tx {
                let _ = tx.send(event.clone());
            }

            tokio::time::sleep(self.behavior.event_delay).await;
        }

        if self.behavior.stall {
            tokio::time::sleep(self.behavior.stall_after).await;
            // Stall indefinitely — the caller should cancel/kill us
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        }

        tokio::time::sleep(self.behavior.turn_duration).await;

        (output_lines, self.behavior.exit_code)
    }

    /// Run multi-turn scenarios.
    async fn run_multi_turn(&self) -> (Vec<String>, i32) {
        let mut output_lines = Vec::new();
        let max_turns = self.behavior.max_turns.unwrap_or(u32::MAX);
        let mut turn_count = 0u32;

        for scenario in &self.behavior.turn_scenarios {
            turn_count += 1;
            if turn_count > max_turns {
                break;
            }

            for event in &scenario.events {
                let line = serde_json::to_string(event).unwrap();
                output_lines.push(line);

                if let Some(tx) = &self.event_tx {
                    let _ = tx.send(event.clone());
                }

                tokio::time::sleep(scenario.event_delay).await;
            }

            // If the turn didn't complete, exit with error
            if !scenario.completes {
                return (output_lines, 1);
            }
        }

        tokio::time::sleep(self.behavior.turn_duration).await;
        (output_lines, self.behavior.exit_code)
    }

    /// Get the configured exit code.
    pub fn exit_code(&self) -> i32 {
        self.behavior.exit_code
    }

    /// Check if this process is configured to stall.
    pub fn will_stall(&self) -> bool {
        self.behavior.stall
    }

    /// Get the number of turn scenarios configured.
    pub fn turn_count(&self) -> usize {
        if self.behavior.turn_scenarios.is_empty() {
            1
        } else {
            self.behavior.turn_scenarios.len()
        }
    }
}

/// Spawn a real subprocess that acts as a fake codex server.
///
/// This creates a shell script on the fly that emits the configured events
/// and exits with the specified code. Useful for tests that need actual
/// process management (kill signals, stdin/stdout pipes).
pub async fn spawn_fake_codex_script(
    behavior: &CodexBehavior,
    work_dir: &std::path::Path,
) -> std::io::Result<Child> {
    let events_json = serde_json::to_string(&behavior.events).unwrap();
    let delay_ms = behavior.event_delay.as_millis();
    let exit_code = behavior.exit_code;
    let stall = behavior.stall;

    // Build a shell script that emits events
    let script = if stall {
        format!(
            r#"#!/bin/bash
EVENTS='{events_json}'
echo "$EVENTS" | python3 -c "
import json, sys, time
events = json.loads(sys.stdin.read())
for e in events:
    print(json.dumps(e), flush=True)
    time.sleep({delay_ms} / 1000.0)
# Stall: sleep forever
while True:
    time.sleep(3600)
"
"#
        )
    } else {
        format!(
            r#"#!/bin/bash
EVENTS='{events_json}'
echo "$EVENTS" | python3 -c "
import json, sys, time
events = json.loads(sys.stdin.read())
for e in events:
    print(json.dumps(e), flush=True)
    time.sleep({delay_ms} / 1000.0)
"
exit {exit_code}
"#
        )
    };

    let script_path = work_dir.join("fake_codex.sh");
    tokio::fs::write(&script_path, &script).await?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&script_path, perms)?;
    }

    let child = Command::new("bash")
        .arg(&script_path)
        .current_dir(work_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    Ok(child)
}

/// Read JSON-line events from a child process stdout.
pub async fn read_codex_events(child: &mut Child) -> Vec<CodexEvent> {
    let stdout = child.stdout.take().expect("child stdout not captured");
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();
    let mut events = Vec::new();

    while let Ok(Some(line)) = lines.next_line().await {
        if let Ok(event) = serde_json::from_str::<CodexEvent>(&line) {
            events.push(event);
        }
    }

    events
}

/// JSON-RPC 2.0 request structure for the fake codex server binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 response structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: serde_json::Value, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fake_codex_success_behavior() {
        let behavior = CodexBehavior::success();
        let process = FakeCodexProcess::new(behavior);

        let (lines, exit_code) = process.run().await;

        assert_eq!(exit_code, 0);
        assert!(!lines.is_empty());

        // Verify first event is turn.start
        let first: CodexEvent = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(first.event_type, "turn.start");

        // Verify last event is turn.end
        let last: CodexEvent = serde_json::from_str(lines.last().unwrap()).unwrap();
        assert_eq!(last.event_type, "turn.end");
    }

    #[tokio::test]
    async fn test_fake_codex_failure_behavior() {
        let behavior = CodexBehavior::failure(1);
        let process = FakeCodexProcess::new(behavior);

        let (lines, exit_code) = process.run().await;

        assert_eq!(exit_code, 1);
        // Should contain an error event
        let has_error = lines.iter().any(|l| l.contains("\"type\":\"error\""));
        assert!(has_error);
    }

    #[tokio::test]
    async fn test_fake_codex_event_observer() {
        let behavior = CodexBehavior::success();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

        tokio::spawn(async move {
            process.run().await;
        });

        // Should receive events through the channel
        let first = rx.recv().await.unwrap();
        assert_eq!(first.event_type, "turn.start");
    }

    #[tokio::test]
    async fn test_fake_codex_stall_is_cancellable() {
        let behavior = CodexBehavior::stalling(Duration::from_millis(10));
        let process = FakeCodexProcess::new(behavior);

        let handle = tokio::spawn(async move {
            process.run().await;
        });

        // Give it time to start stalling
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Should be able to abort it
        handle.abort();
        let result = handle.await;
        assert!(result.is_err()); // JoinError from abort
    }

    #[tokio::test]
    async fn test_fake_codex_multi_turn() {
        let behavior = CodexBehavior::n_turns(3);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

        tokio::spawn(async move {
            process.run().await;
        });

        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }

        // 3 turns, each with 4 events (turn_start, message, token_usage, turn_completed)
        assert_eq!(events.len(), 12);

        // Verify turn boundaries
        let turn_starts: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == "turn.start")
            .collect();
        assert_eq!(turn_starts.len(), 3);
    }

    #[tokio::test]
    async fn test_fake_codex_max_turns_limit() {
        let behavior = CodexBehavior::hits_max_turns(2);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

        tokio::spawn(async move {
            process.run().await;
        });

        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }

        // Should only emit events for 2 turns (max_turns=2)
        let turn_starts: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == "turn.start")
            .collect();
        assert_eq!(turn_starts.len(), 2);
    }

    #[tokio::test]
    async fn test_fake_codex_approval_scenario() {
        let behavior = CodexBehavior::with_approval_on_turn(3, 2);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

        tokio::spawn(async move {
            process.run().await;
        });

        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }

        // Turn 1 succeeds (4 events), turn 2 has approval request (3 events), then exits
        let has_approval = events.iter().any(|e| e.event_type == "approval_request");
        assert!(has_approval);
    }

    #[tokio::test]
    async fn test_fake_codex_tool_call_scenario() {
        let behavior = CodexBehavior::with_tool_call_on_turn(2, 1);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let process = FakeCodexProcess::new(behavior).with_event_observer(tx);

        tokio::spawn(async move {
            process.run().await;
        });

        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }

        let has_tool_call = events.iter().any(|e| e.event_type == "tool_call");
        let has_tool_result = events.iter().any(|e| e.event_type == "tool_result");
        assert!(has_tool_call);
        assert!(has_tool_result);
    }

    #[tokio::test]
    async fn test_json_rpc_response_success() {
        let resp = JsonRpcResponse::success(
            serde_json::json!(1),
            serde_json::json!({"thread_id": "t-1"}),
        );
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"thread_id\":\"t-1\""));
        assert!(!json.contains("\"error\""));
    }

    #[tokio::test]
    async fn test_json_rpc_response_error() {
        let resp = JsonRpcResponse::error(serde_json::json!(2), -32600, "Invalid request");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"code\":-32600"));
        assert!(json.contains("Invalid request"));
        assert!(!json.contains("\"result\""));
    }
}
