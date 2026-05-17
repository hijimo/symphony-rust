//! Scheduler — tick scheduling and dispatch loop.
//!
//! Implements the poll-and-dispatch tick logic from SPEC sections 8.1-8.2.
//! The scheduler is responsible for:
//! - Scheduling periodic ticks
//! - Running the dispatch loop with slot checking
//! - Candidate eligibility checks (should_dispatch)
//! - Sort-for-dispatch ordering

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::models::{normalize_state, Issue, OrchestratorEvent, OrchestratorState};

/// Configuration values needed for dispatch decisions.
/// Extracted from ServiceConfig to avoid tight coupling.
#[derive(Debug, Clone)]
pub struct DispatchConfig {
    pub active_states: Vec<String>,
    pub terminal_states: Vec<String>,
    pub max_concurrent_agents: usize,
    pub max_concurrent_agents_by_state: HashMap<String, usize>,
    pub blocker_check_states: Vec<String>,
    pub poll_interval_ms: u64,
    /// Optional assignee filter: only dispatch issues assigned to this worker instance.
    pub assignee_id: Option<String>,
}

impl Default for DispatchConfig {
    fn default() -> Self {
        Self {
            active_states: vec!["Todo".to_string(), "In Progress".to_string()],
            terminal_states: vec![
                "Closed".to_string(),
                "Cancelled".to_string(),
                "Canceled".to_string(),
                "Duplicate".to_string(),
                "Done".to_string(),
            ],
            max_concurrent_agents: 10,
            max_concurrent_agents_by_state: HashMap::new(),
            blocker_check_states: vec!["todo".to_string()],
            poll_interval_ms: 30_000,
            assignee_id: None,
        }
    }
}

impl DispatchConfig {
    /// Derive a DispatchConfig from a ServiceConfig.
    ///
    /// Maps the typed service configuration values into the dispatch-specific
    /// subset needed by the scheduler.
    pub fn from_service_config(sc: &crate::config::service_config::ServiceConfig) -> Self {
        Self {
            active_states: sc.active_states.clone(),
            terminal_states: sc.terminal_states.clone(),
            max_concurrent_agents: sc.max_concurrent_agents,
            max_concurrent_agents_by_state: sc.max_concurrent_agents_by_state.clone(),
            blocker_check_states: sc.blocker_check_states.clone(),
            poll_interval_ms: sc.poll_interval_ms,
            assignee_id: None, // Set externally by the caller
        }
    }
}

/// Schedule the next tick after the given interval.
/// Spawns a timer task that sends a Tick event when it fires.
pub fn schedule_next_tick(
    event_tx: &mpsc::Sender<OrchestratorEvent>,
    interval_ms: u64,
) -> tokio::task::JoinHandle<()> {
    let tx = event_tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        let _ = tx.send(OrchestratorEvent::Tick).await;
    })
}

/// Schedule an immediate tick (used at startup).
pub fn schedule_immediate_tick(
    event_tx: &mpsc::Sender<OrchestratorEvent>,
) -> tokio::task::JoinHandle<()> {
    let tx = event_tx.clone();
    tokio::spawn(async move {
        let _ = tx.send(OrchestratorEvent::Tick).await;
    })
}

/// Check if an issue is dispatch-eligible (SPEC section 8.2).
///
/// An issue is eligible only if ALL of the following are true:
/// 1. It has id, identifier, title, and state
/// 2. Its state is in active_states and NOT in terminal_states
/// 3. It is not already in running
/// 4. It is not already in claimed
/// 5. Global concurrency slots are available
/// 6. Per-state concurrency slots are available
/// 7. Blocker rule passes (for configured blocker_check_states)
pub fn should_dispatch(issue: &Issue, state: &OrchestratorState, config: &DispatchConfig) -> bool {
    // 1. Required fields check
    if issue.id.is_empty()
        || issue.identifier.is_empty()
        || issue.title.is_empty()
        || issue.state.is_empty()
    {
        return false;
    }

    let normalized_issue_state = normalize_state(&issue.state);

    // 2. State must be active and not terminal
    let is_active = config
        .active_states
        .iter()
        .any(|s| normalize_state(s) == normalized_issue_state);
    if !is_active {
        return false;
    }

    let is_terminal = config
        .terminal_states
        .iter()
        .any(|s| normalize_state(s) == normalized_issue_state);
    if is_terminal {
        return false;
    }

    // 3. Not already running
    if state.running.contains_key(&issue.id) {
        return false;
    }

    // 4. Not already claimed
    if state.claimed.contains(&issue.id) {
        return false;
    }

    // 5. Global concurrency slots available
    if !state.has_global_slots() {
        return false;
    }

    // 6. Per-state concurrency slots available
    if !available_state_slots(&normalized_issue_state, state, config) {
        return false;
    }

    // 7. Blocker rule for configured states
    if config
        .blocker_check_states
        .contains(&normalized_issue_state)
    {
        let has_active_blocker = issue.blocked_by.iter().any(|b| {
            b.state
                .as_ref()
                .map(|s| {
                    !config
                        .terminal_states
                        .iter()
                        .any(|ts| normalize_state(ts) == normalize_state(s))
                })
                .unwrap_or(true) // Unknown state treated as non-terminal
        });
        if has_active_blocker {
            return false;
        }
    }

    true
}

/// Check per-state concurrency slot availability (SPEC section 8.3).
fn available_state_slots(
    normalized_issue_state: &str,
    state: &OrchestratorState,
    config: &DispatchConfig,
) -> bool {
    if let Some(&limit) = config
        .max_concurrent_agents_by_state
        .get(normalized_issue_state)
    {
        let count = state
            .running
            .values()
            .filter(|e| normalize_state(&e.issue.state) == normalized_issue_state)
            .count();
        count < limit
    } else {
        true // No per-state limit configured
    }
}

/// Sort issues for dispatch priority (SPEC section 8.2).
///
/// Sorting order:
/// 1. priority ascending (lower = higher priority; None sorts last)
/// 2. created_at oldest first
/// 3. identifier lexicographic tie-breaker
pub fn sort_for_dispatch(issues: &mut [Issue]) {
    issues.sort_by(|a, b| {
        let pa = a.priority.unwrap_or(i32::MAX);
        let pb = b.priority.unwrap_or(i32::MAX);
        pa.cmp(&pb)
            .then_with(|| a.created_at.cmp(&b.created_at))
            .then_with(|| a.identifier.cmp(&b.identifier))
    });
}

/// Check if a state is in the terminal states list.
pub fn is_terminal_state(state: &str, terminal_states: &[String]) -> bool {
    let normalized = normalize_state(state);
    terminal_states
        .iter()
        .any(|ts| normalize_state(ts) == normalized)
}

/// Check if a state is in the active states list.
pub fn is_active_state(state: &str, active_states: &[String]) -> bool {
    let normalized = normalize_state(state);
    active_states
        .iter()
        .any(|s| normalize_state(s) == normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{BlockerRef, OrchestratorState};

    fn make_issue(id: &str, state: &str, priority: Option<i32>) -> Issue {
        Issue {
            id: id.to_string(),
            identifier: format!("TEST-{}", id),
            title: format!("Test issue {}", id),
            description: None,
            priority,
            state: state.to_string(),
            branch_name: None,
            url: None,
            labels: vec![],
            blocked_by: vec![],
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn test_should_dispatch_basic_eligible() {
        let state = OrchestratorState::new(30_000, 10);
        let config = DispatchConfig::default();
        let issue = make_issue("1", "Todo", Some(1));
        assert!(should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_should_dispatch_terminal_state_rejected() {
        let state = OrchestratorState::new(30_000, 10);
        let config = DispatchConfig::default();
        let issue = make_issue("1", "Done", Some(1));
        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_should_dispatch_already_claimed() {
        let mut state = OrchestratorState::new(30_000, 10);
        state.claimed.insert("1".to_string());
        let config = DispatchConfig::default();
        let issue = make_issue("1", "Todo", Some(1));
        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_should_dispatch_no_global_slots() {
        let state = OrchestratorState::new(30_000, 0);
        let config = DispatchConfig {
            max_concurrent_agents: 0,
            ..DispatchConfig::default()
        };
        let issue = make_issue("1", "Todo", Some(1));
        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_should_dispatch_blocked_in_todo() {
        let state = OrchestratorState::new(30_000, 10);
        let config = DispatchConfig::default();
        let mut issue = make_issue("1", "Todo", Some(1));
        issue.blocked_by = vec![BlockerRef {
            id: Some("2".to_string()),
            identifier: Some("TEST-2".to_string()),
            state: Some("In Progress".to_string()), // non-terminal
        }];
        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_should_dispatch_blocked_but_blocker_terminal() {
        let state = OrchestratorState::new(30_000, 10);
        let config = DispatchConfig::default();
        let mut issue = make_issue("1", "Todo", Some(1));
        issue.blocked_by = vec![BlockerRef {
            id: Some("2".to_string()),
            identifier: Some("TEST-2".to_string()),
            state: Some("Done".to_string()), // terminal
        }];
        assert!(should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_sort_for_dispatch() {
        use chrono::{TimeZone, Utc};
        let mut issues = vec![
            {
                let mut i = make_issue("3", "Todo", Some(2));
                i.created_at = Some(Utc.with_ymd_and_hms(2024, 1, 3, 0, 0, 0).unwrap());
                i
            },
            {
                let mut i = make_issue("1", "Todo", Some(1));
                i.created_at = Some(Utc.with_ymd_and_hms(2024, 1, 2, 0, 0, 0).unwrap());
                i
            },
            {
                let mut i = make_issue("2", "Todo", Some(1));
                i.created_at = Some(Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
                i
            },
        ];

        sort_for_dispatch(&mut issues);

        // Priority 1 before priority 2
        assert_eq!(issues[0].id, "2"); // priority 1, oldest
        assert_eq!(issues[1].id, "1"); // priority 1, newer
        assert_eq!(issues[2].id, "3"); // priority 2
    }

    #[test]
    fn test_is_terminal_state() {
        let terminals = vec!["Done".to_string(), "Closed".to_string()];
        assert!(is_terminal_state("done", &terminals));
        assert!(is_terminal_state("Done", &terminals));
        assert!(!is_terminal_state("In Progress", &terminals));
    }
}
