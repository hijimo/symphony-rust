//! MockCodexProcess — simulates the Codex app-server JSON-line protocol.
//!
//! Used for integration tests that need to simulate agent sessions without
//! actually spawning a real Codex process.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

/// Result of a simulated turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnResult {
    /// Turn completed successfully
    Success,
    /// Turn failed with an error
    Failed(String),
    /// Turn timed out
    Timeout,
    /// Process exited normally (continuation)
    NormalExit,
    /// Process exited abnormally
    AbnormalExit(i32),
}

/// A simulated Codex event in the JSON-line protocol.
#[derive(Debug, Clone)]
pub struct CodexEvent {
    pub event_type: String,
    pub payload: Value,
    /// Delay before emitting this event (simulates processing time)
    pub delay: Duration,
}

impl CodexEvent {
    /// Create a session_started event.
    pub fn session_started(thread_id: &str, turn_id: &str) -> Self {
        Self {
            event_type: "session_started".to_string(),
            payload: json!({
                "thread_id": thread_id,
                "turn_id": turn_id,
            }),
            delay: Duration::from_millis(10),
        }
    }

    /// Create a turn_started event.
    pub fn turn_started(turn_id: &str) -> Self {
        Self {
            event_type: "turn_started".to_string(),
            payload: json!({
                "turn_id": turn_id,
            }),
            delay: Duration::from_millis(5),
        }
    }

    /// Create a usage/token event.
    pub fn usage(input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            event_type: "usage".to_string(),
            payload: json!({
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "total_tokens": input_tokens + output_tokens,
            }),
            delay: Duration::from_millis(5),
        }
    }

    /// Create a turn_completed event.
    pub fn turn_completed(turn_id: &str) -> Self {
        Self {
            event_type: "turn_completed".to_string(),
            payload: json!({
                "turn_id": turn_id,
                "status": "completed",
            }),
            delay: Duration::from_millis(5),
        }
    }

    /// Create a session_ended event.
    pub fn session_ended(reason: &str) -> Self {
        Self {
            event_type: "session_ended".to_string(),
            payload: json!({
                "reason": reason,
            }),
            delay: Duration::from_millis(5),
        }
    }

    /// Create a custom event with a delay (for timeout simulation).
    pub fn delayed(event_type: &str, payload: Value, delay: Duration) -> Self {
        Self {
            event_type: event_type.to_string(),
            payload,
            delay,
        }
    }
}

/// Configuration for a mock Codex process.
#[derive(Debug, Clone)]
pub struct MockCodexConfig {
    /// Events to emit in sequence
    pub events: Vec<CodexEvent>,
    /// Final turn result
    pub turn_result: TurnResult,
    /// Whether to simulate a stall (stop emitting events)
    pub simulate_stall: bool,
    /// Duration to stall before resuming (if simulate_stall is true)
    pub stall_duration: Duration,
}

impl Default for MockCodexConfig {
    fn default() -> Self {
        Self {
            events: vec![
                CodexEvent::session_started("thread-1", "turn-1"),
                CodexEvent::turn_started("turn-1"),
                CodexEvent::usage(1000, 500),
                CodexEvent::turn_completed("turn-1"),
                CodexEvent::session_ended("completed"),
            ],
            turn_result: TurnResult::NormalExit,
            simulate_stall: false,
            stall_duration: Duration::from_secs(0),
        }
    }
}

/// A mock Codex process that simulates the app-server protocol.
///
/// Supports:
/// - Configurable event sequences
/// - Timeout simulation
/// - Stall simulation
/// - Multiple turn scenarios
pub struct MockCodexProcess {
    config: MockCodexConfig,
    events_queue: Arc<Mutex<VecDeque<CodexEvent>>>,
    emitted_events: Arc<Mutex<Vec<CodexEvent>>>,
    is_running: Arc<Mutex<bool>>,
    exit_code: Arc<Mutex<Option<i32>>>,
}

impl MockCodexProcess {
    /// Create a new mock process with the given configuration.
    pub fn new(config: MockCodexConfig) -> Self {
        let events_queue: VecDeque<CodexEvent> = config.events.clone().into_iter().collect();
        Self {
            config,
            events_queue: Arc::new(Mutex::new(events_queue)),
            emitted_events: Arc::new(Mutex::new(Vec::new())),
            is_running: Arc::new(Mutex::new(false)),
            exit_code: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a mock that simulates a successful single-turn session.
    pub fn successful_session() -> Self {
        Self::new(MockCodexConfig::default())
    }

    /// Create a mock that simulates a failed session.
    pub fn failed_session(error: &str) -> Self {
        Self::new(MockCodexConfig {
            events: vec![
                CodexEvent::session_started("thread-1", "turn-1"),
                CodexEvent::turn_started("turn-1"),
                CodexEvent::session_ended("error"),
            ],
            turn_result: TurnResult::Failed(error.to_string()),
            ..Default::default()
        })
    }

    /// Create a mock that simulates a timeout.
    pub fn timeout_session(stall_duration: Duration) -> Self {
        Self::new(MockCodexConfig {
            events: vec![
                CodexEvent::session_started("thread-1", "turn-1"),
                CodexEvent::turn_started("turn-1"),
                CodexEvent::delayed(
                    "stall",
                    json!({"message": "simulated stall"}),
                    stall_duration,
                ),
            ],
            turn_result: TurnResult::Timeout,
            simulate_stall: true,
            stall_duration,
        })
    }

    /// Create a mock that simulates a multi-turn session.
    pub fn multi_turn_session(num_turns: u32) -> Self {
        let mut events = Vec::new();
        events.push(CodexEvent::session_started("thread-1", "turn-1"));

        for i in 1..=num_turns {
            let turn_id = format!("turn-{}", i);
            events.push(CodexEvent::turn_started(&turn_id));
            events.push(CodexEvent::usage(500 * i as u64, 250 * i as u64));
            events.push(CodexEvent::turn_completed(&turn_id));
        }

        events.push(CodexEvent::session_ended("completed"));

        Self::new(MockCodexConfig {
            events,
            turn_result: TurnResult::NormalExit,
            ..Default::default()
        })
    }

    /// Start the mock process (marks it as running).
    pub fn start(&self) {
        *self.is_running.lock().unwrap() = true;
    }

    /// Get the next event from the queue.
    pub fn next_event(&self) -> Option<CodexEvent> {
        let mut queue = self.events_queue.lock().unwrap();
        if let Some(event) = queue.pop_front() {
            self.emitted_events.lock().unwrap().push(event.clone());
            Some(event)
        } else {
            None
        }
    }

    /// Check if the process is still running.
    pub fn is_running(&self) -> bool {
        *self.is_running.lock().unwrap()
    }

    /// Kill the process (simulates SIGKILL).
    pub fn kill(&self) {
        *self.is_running.lock().unwrap() = false;
        *self.exit_code.lock().unwrap() = Some(-9);
    }

    /// Stop the process normally.
    pub fn stop(&self) {
        *self.is_running.lock().unwrap() = false;
        let code = match &self.config.turn_result {
            TurnResult::Success | TurnResult::NormalExit => 0,
            TurnResult::AbnormalExit(code) => *code,
            TurnResult::Failed(_) => 1,
            TurnResult::Timeout => -1,
        };
        *self.exit_code.lock().unwrap() = Some(code);
    }

    /// Get the exit code (None if still running).
    pub fn exit_code(&self) -> Option<i32> {
        *self.exit_code.lock().unwrap()
    }

    /// Get all emitted events so far.
    pub fn emitted_events(&self) -> Vec<CodexEvent> {
        self.emitted_events.lock().unwrap().clone()
    }

    /// Get the configured turn result.
    pub fn turn_result(&self) -> &TurnResult {
        &self.config.turn_result
    }

    /// Check if this is configured to simulate a stall.
    pub fn will_stall(&self) -> bool {
        self.config.simulate_stall
    }

    /// Get the total tokens from all usage events.
    pub fn total_tokens(&self) -> (u64, u64) {
        let events = self.emitted_events.lock().unwrap();
        let mut input = 0u64;
        let mut output = 0u64;
        for event in events.iter() {
            if event.event_type == "usage" {
                input += event.payload["input_tokens"].as_u64().unwrap_or(0);
                output += event.payload["output_tokens"].as_u64().unwrap_or(0);
            }
        }
        (input, output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_successful_session_events() {
        let mock = MockCodexProcess::successful_session();
        mock.start();

        let mut events = Vec::new();
        while let Some(event) = mock.next_event() {
            events.push(event);
        }

        assert_eq!(events.len(), 5);
        assert_eq!(events[0].event_type, "session_started");
        assert_eq!(events[1].event_type, "turn_started");
        assert_eq!(events[2].event_type, "usage");
        assert_eq!(events[3].event_type, "turn_completed");
        assert_eq!(events[4].event_type, "session_ended");
    }

    #[test]
    fn test_multi_turn_session() {
        let mock = MockCodexProcess::multi_turn_session(3);
        mock.start();

        let mut events = Vec::new();
        while let Some(event) = mock.next_event() {
            events.push(event);
        }

        // session_started + 3*(turn_started + usage + turn_completed) + session_ended
        assert_eq!(events.len(), 1 + 3 * 3 + 1);
    }

    #[test]
    fn test_kill_process() {
        let mock = MockCodexProcess::successful_session();
        mock.start();
        assert!(mock.is_running());

        mock.kill();
        assert!(!mock.is_running());
        assert_eq!(mock.exit_code(), Some(-9));
    }

    #[test]
    fn test_token_accounting() {
        let mock = MockCodexProcess::multi_turn_session(2);
        mock.start();

        while mock.next_event().is_some() {}

        let (input, output) = mock.total_tokens();
        // Turn 1: 500 input, 250 output; Turn 2: 1000 input, 500 output
        assert_eq!(input, 1500);
        assert_eq!(output, 750);
    }
}
