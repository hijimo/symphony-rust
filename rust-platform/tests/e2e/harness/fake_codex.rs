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

    /// Get the configured exit code.
    pub fn exit_code(&self) -> i32 {
        self.behavior.exit_code
    }

    /// Check if this process is configured to stall.
    pub fn will_stall(&self) -> bool {
        self.behavior.stall
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
}
