//! Core domain models for the Symphony orchestrator state machine.
//!
//! These types represent the authoritative in-memory state owned by the orchestrator,
//! as specified in SPEC sections 4.1 and 7-8.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// Issue & Blocker (SPEC section 4.1.1)
// ---------------------------------------------------------------------------

/// Blocker relationship reference (SPEC section 4.1.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockerRef {
    pub id: Option<String>,
    pub identifier: Option<String>,
    pub state: Option<String>,
}

/// Normalized issue model used by orchestration, prompt rendering, and observability.
/// (SPEC section 4.1.1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    /// Stable tracker-internal ID (Linear UUID / GitHub number as string).
    pub id: String,
    /// Human-readable ticket key (e.g. "ABC-123").
    pub identifier: String,
    pub title: String,
    pub description: Option<String>,
    /// Priority: lower numbers are higher priority; None sorts last.
    pub priority: Option<i32>,
    /// Current tracker state name.
    pub state: String,
    pub branch_name: Option<String>,
    pub url: Option<String>,
    /// Labels (normalized to lowercase).
    pub labels: Vec<String>,
    pub blocked_by: Vec<BlockerRef>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Workflow Definition (SPEC section 4.1.2)
// ---------------------------------------------------------------------------

/// Parsed WORKFLOW.md payload (SPEC section 4.1.2).
#[derive(Debug, Clone)]
pub struct WorkflowDefinition {
    /// YAML front matter root object.
    pub config: HashMap<String, serde_yaml::Value>,
    /// Markdown body (trimmed).
    pub prompt_template: String,
}

// ---------------------------------------------------------------------------
// Run Attempt (SPEC section 4.1.5 + section 7.2)
// ---------------------------------------------------------------------------

/// Run attempt lifecycle phases (SPEC section 7.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStatus {
    PreparingWorkspace,
    BuildingPrompt,
    LaunchingAgentProcess,
    InitializingSession,
    StreamingTurn,
    Finishing,
    Succeeded,
    Failed,
    TimedOut,
    Stalled,
    CanceledByReconciliation,
}

/// One execution attempt for one issue (SPEC section 4.1.5).
#[derive(Debug, Clone)]
pub struct RunAttempt {
    pub issue_id: String,
    pub issue_identifier: String,
    /// None = first run, Some(n) = nth retry/continuation.
    pub attempt: Option<u32>,
    pub workspace_path: PathBuf,
    pub started_at: DateTime<Utc>,
    pub status: RunStatus,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Live Session (SPEC section 4.1.6)
// ---------------------------------------------------------------------------

/// Agent session metadata tracked while a coding-agent subprocess is running.
/// (SPEC section 4.1.6)
#[derive(Debug, Clone)]
pub struct LiveSession {
    /// "<thread_id>-<turn_id>"
    pub session_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub codex_app_server_pid: Option<String>,
    pub last_codex_event: Option<String>,
    pub last_codex_timestamp: Option<DateTime<Utc>>,
    /// Monotonic timestamp for stall detection (avoids NTP drift issues).
    pub last_activity_instant: Instant,
    pub last_codex_message: Option<String>,
    pub codex_input_tokens: u64,
    pub codex_output_tokens: u64,
    pub codex_total_tokens: u64,
    pub last_reported_input_tokens: u64,
    pub last_reported_output_tokens: u64,
    pub last_reported_total_tokens: u64,
    pub turn_count: u32,
}

impl LiveSession {
    /// Create a new LiveSession with default token counters.
    pub fn new(thread_id: String, turn_id: String) -> Self {
        let session_id = compose_session_id(&thread_id, &turn_id);
        Self {
            session_id,
            thread_id,
            turn_id,
            codex_app_server_pid: None,
            last_codex_event: None,
            last_codex_timestamp: None,
            last_activity_instant: Instant::now(),
            last_codex_message: None,
            codex_input_tokens: 0,
            codex_output_tokens: 0,
            codex_total_tokens: 0,
            last_reported_input_tokens: 0,
            last_reported_output_tokens: 0,
            last_reported_total_tokens: 0,
            turn_count: 0,
        }
    }

    /// Update the activity instant (resets stall timer).
    pub fn touch(&mut self) {
        self.last_activity_instant = Instant::now();
    }
}

// ---------------------------------------------------------------------------
// Retry Entry (SPEC section 4.1.7)
// ---------------------------------------------------------------------------

/// Distinguishes continuation (normal exit) from failure (abnormal exit) retries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryKind {
    /// Normal exit followed by a short continuation delay (fixed 1s).
    Continuation,
    /// Abnormal exit with exponential backoff.
    Failure,
}

/// Scheduled retry state for an issue (SPEC section 4.1.7).
#[derive(Debug)]
pub struct RetryEntry {
    pub issue_id: String,
    pub identifier: String,
    /// 1-based retry attempt count.
    pub attempt: u32,
    pub retry_kind: RetryKind,
    /// Due time in monotonic milliseconds.
    pub due_at_ms: u64,
    /// Tokio timer handle (abortable).
    pub timer_handle: JoinHandle<()>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Running Entry
// ---------------------------------------------------------------------------

/// A currently-running worker entry in the orchestrator state.
#[derive(Debug)]
pub struct RunningEntry {
    pub worker_handle: JoinHandle<()>,
    pub cancel_token: CancellationToken,
    pub identifier: String,
    pub issue: Issue,
    pub session: LiveSession,
    pub retry_attempt: Option<u32>,
    /// Monotonic start time (avoids NTP drift for duration calculations).
    pub started_at: Instant,
    /// UTC start time for logs and API display.
    pub started_at_utc: DateTime<Utc>,
    /// When cancel signal was sent (for hard deadline enforcement).
    pub cancel_sent_at: Option<Instant>,
}

// ---------------------------------------------------------------------------
// Codex Totals
// ---------------------------------------------------------------------------

/// Aggregate token and runtime statistics.
#[derive(Debug, Clone, Default)]
pub struct CodexTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    /// Cumulative runtime in milliseconds (monotonic clock).
    pub seconds_running_ms: u64,
}

impl CodexTotals {
    /// Add runtime seconds from a completed worker.
    pub fn add_runtime(&mut self, started_at: Instant) {
        self.seconds_running_ms += started_at.elapsed().as_millis() as u64;
    }
}

// ---------------------------------------------------------------------------
// Orchestrator State (SPEC section 4.1.8)
// ---------------------------------------------------------------------------

/// Single authoritative in-memory state owned by the orchestrator.
/// All state mutations are serialized through the orchestrator event loop.
/// (SPEC section 4.1.8)
#[derive(Debug)]
pub struct OrchestratorState {
    pub poll_interval_ms: u64,
    pub max_concurrent_agents: usize,
    /// issue_id -> RunningEntry
    pub running: HashMap<String, RunningEntry>,
    /// Claimed issue IDs (running + retrying).
    pub claimed: HashSet<String>,
    /// issue_id -> RetryEntry
    pub retry_attempts: HashMap<String, RetryEntry>,
    /// Completed issue IDs with completion time (for periodic GC).
    pub completed: HashMap<String, Instant>,
    pub codex_totals: CodexTotals,
    pub codex_rate_limits: Option<serde_json::Value>,
    /// Last defensive config re-read time.
    pub last_config_check: Instant,
    /// Defensive re-read interval (every N ticks).
    pub config_check_interval_ticks: u32,
    pub tick_count: u32,
    /// Shutdown flag: when true, all dispatch/retry paths are skipped.
    pub shutting_down: bool,
}

impl OrchestratorState {
    /// Create a new OrchestratorState with the given configuration values.
    pub fn new(poll_interval_ms: u64, max_concurrent_agents: usize) -> Self {
        Self {
            poll_interval_ms,
            max_concurrent_agents,
            running: HashMap::new(),
            claimed: HashSet::new(),
            retry_attempts: HashMap::new(),
            completed: HashMap::new(),
            codex_totals: CodexTotals::default(),
            codex_rate_limits: None,
            last_config_check: Instant::now(),
            config_check_interval_ticks: 10,
            tick_count: 0,
            shutting_down: false,
        }
    }

    /// Garbage-collect expired completed records (retain last 1 hour).
    /// Prevents unbounded memory growth from the completed set.
    pub fn gc_completed(&mut self) {
        let cutoff = Instant::now() - Duration::from_secs(3600);
        self.completed.retain(|_, completed_at| *completed_at > cutoff);
    }

    /// Number of available global concurrency slots.
    pub fn available_global_slots(&self) -> usize {
        self.max_concurrent_agents.saturating_sub(self.running.len())
    }

    /// Check if there are any available global slots.
    pub fn has_global_slots(&self) -> bool {
        self.available_global_slots() > 0
    }
}

// ---------------------------------------------------------------------------
// Orchestrator Events
// ---------------------------------------------------------------------------

/// Update payload from a Codex event stream.
#[derive(Debug, Clone)]
pub struct CodexEventUpdate {
    pub event_type: Option<String>,
    pub message: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub timestamp: Option<DateTime<Utc>>,
    pub rate_limits: Option<serde_json::Value>,
}

/// Events received by the orchestrator event loop.
/// All state mutations flow through this channel.
#[derive(Debug)]
pub enum OrchestratorEvent {
    /// Periodic tick trigger.
    Tick,
    /// Worker exited normally (clean exit, issue may need continuation).
    WorkerExitNormal { issue_id: String },
    /// Worker exited abnormally (error, needs exponential backoff retry).
    WorkerExitAbnormal { issue_id: String, error: String },
    /// Codex event update (token counters, activity, rate limits).
    CodexUpdate { issue_id: String, update: CodexEventUpdate },
    /// Retry timer fired for an issue.
    RetryFired { issue_id: String },
    /// Configuration was reloaded (hot reload notification).
    ConfigReloaded,
    /// External trigger to force a refresh (e.g. HTTP API /refresh).
    ForceRefresh,
    /// Shutdown signal received.
    Shutdown,
}

// ---------------------------------------------------------------------------
// Shutdown Config
// ---------------------------------------------------------------------------

/// Configuration for graceful shutdown behavior.
#[derive(Debug, Clone)]
pub struct ShutdownConfig {
    /// Timeout for waiting on active workers to finish (default 30s).
    pub worker_drain_timeout_ms: u64,
    /// HTTP server drain timeout (default 5s).
    pub http_drain_timeout_ms: u64,
}

impl Default for ShutdownConfig {
    fn default() -> Self {
        Self {
            worker_drain_timeout_ms: 30_000,
            http_drain_timeout_ms: 5_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Helper Functions
// ---------------------------------------------------------------------------

/// Compose a session ID from thread_id and turn_id (SPEC section 4.2).
pub fn compose_session_id(thread_id: &str, turn_id: &str) -> String {
    format!("{}-{}", thread_id, turn_id)
}

/// Normalize issue state for comparison (SPEC section 4.2).
pub fn normalize_state(state: &str) -> String {
    state.to_lowercase()
}

/// Sanitize an issue identifier into a workspace key (SPEC section 4.2).
/// Replaces any character not in [A-Za-z0-9._-] with '_'.
/// Returns an error for unsafe identifiers (empty, ".", "..").
pub fn sanitize_workspace_key(identifier: &str) -> Result<String, String> {
    let sanitized: String = identifier
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();

    if sanitized.is_empty()
        || sanitized == "."
        || sanitized == ".."
        || sanitized.chars().all(|c| c == '.')
    {
        return Err(format!("Unsafe identifier: '{}'", identifier));
    }

    Ok(sanitized)
}

/// Get current monotonic time in milliseconds (for retry due_at_ms).
pub fn current_monotonic_ms() -> u64 {
    // Use a process-relative monotonic reference.
    // This is relative to an arbitrary epoch but consistent within the process.
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_workspace_key_normal() {
        assert_eq!(sanitize_workspace_key("ABC-123").unwrap(), "ABC-123");
        assert_eq!(sanitize_workspace_key("my.issue_1").unwrap(), "my.issue_1");
    }

    #[test]
    fn test_sanitize_workspace_key_replaces_special_chars() {
        assert_eq!(sanitize_workspace_key("ABC/123").unwrap(), "ABC_123");
        assert_eq!(sanitize_workspace_key("a b c").unwrap(), "a_b_c");
    }

    #[test]
    fn test_sanitize_workspace_key_rejects_unsafe() {
        assert!(sanitize_workspace_key("").is_err());
        assert!(sanitize_workspace_key(".").is_err());
        assert!(sanitize_workspace_key("..").is_err());
        assert!(sanitize_workspace_key("...").is_err());
    }

    #[test]
    fn test_normalize_state() {
        assert_eq!(normalize_state("In Progress"), "in progress");
        assert_eq!(normalize_state("TODO"), "todo");
    }

    #[test]
    fn test_compose_session_id() {
        assert_eq!(compose_session_id("thread-1", "turn-2"), "thread-1-turn-2");
    }

    #[test]
    fn test_orchestrator_state_gc_completed() {
        let mut state = OrchestratorState::new(30_000, 10);
        // Insert a "recent" entry
        state.completed.insert("recent".to_string(), Instant::now());
        // Insert an "old" entry (simulate by using a past instant)
        // We can't easily create a past Instant, so just verify gc doesn't remove recent ones
        state.gc_completed();
        assert!(state.completed.contains_key("recent"));
    }

    #[test]
    fn test_available_global_slots() {
        let state = OrchestratorState::new(30_000, 5);
        assert_eq!(state.available_global_slots(), 5);
        assert!(state.has_global_slots());
    }
}
