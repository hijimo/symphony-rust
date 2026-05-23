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

//! Unit tests for reconciler logic.
//!
//! Tests cover:
//! - Stall detection (last_activity > stall_timeout)
//! - Two-phase cancel (cancel signal -> 30s hard kill)
//! - Part B: terminal issue worker termination
//! - Missing/invisible issue handling
//! - Disabled stall detection (stall_timeout_ms <= 0)

use std::time::Instant;

use chrono::Utc;
use tokio_util::sync::CancellationToken;

use symphony_platform::models::{Issue, LiveSession, OrchestratorState, RunningEntry};
use symphony_platform::orchestrator::reconciler::{
    determine_reconcile_actions, reconcile_stalled_runs, terminate_running_entry, ReconcileAction,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn make_issue(id: &str, state: &str) -> Issue {
    Issue {
        id: id.to_string(),
        identifier: format!("TEST-{}", id),
        title: format!("Test issue {}", id),
        description: None,
        priority: Some(1),
        state: state.to_string(),
        branch_name: None,
        url: None,
        labels: vec![],
        blocked_by: vec![],
        created_at: None,
        updated_at: None,
    }
}

fn make_running_entry_with_stale_activity(issue: Issue, stale_ms: u64) -> RunningEntry {
    let mut session = LiveSession::new("thread-1".to_string(), "turn-1".to_string());
    // Simulate stale activity by setting last_activity_instant to the past
    session.last_activity_instant = Instant::now() - std::time::Duration::from_millis(stale_ms);

    RunningEntry {
        worker_handle: tokio::spawn(async {}),
        cancel_token: CancellationToken::new(),
        identifier: issue.identifier.clone(),
        issue,
        session,
        retry_attempt: None,
        started_at: Instant::now(),
        started_at_utc: Utc::now(),
        cancel_sent_at: None,
    }
}

fn make_running_entry_cancelled(issue: Issue, cancel_elapsed_ms: u64) -> RunningEntry {
    let mut session = LiveSession::new("thread-1".to_string(), "turn-1".to_string());
    session.last_activity_instant = Instant::now(); // Recent activity (doesn't matter, already cancelled)

    RunningEntry {
        worker_handle: tokio::spawn(async {}),
        cancel_token: CancellationToken::new(),
        identifier: issue.identifier.clone(),
        issue,
        session,
        retry_attempt: Some(2),
        started_at: Instant::now(),
        started_at_utc: Utc::now(),
        cancel_sent_at: Some(Instant::now() - std::time::Duration::from_millis(cancel_elapsed_ms)),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Stall Detection Tests (Part A)
// ═══════════════════════════════════════════════════════════════════════════════

mod stall_detection {
    use super::*;

    #[tokio::test]
    async fn test_stall_detection_disabled_when_timeout_zero() {
        let mut state = OrchestratorState::new(30_000, 10);
        let result = reconcile_stalled_runs(&mut state, 0);
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_stall_detection_disabled_when_timeout_negative() {
        let mut state = OrchestratorState::new(30_000, 10);
        let result = reconcile_stalled_runs(&mut state, -1);
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_no_stall_when_activity_recent() {
        let mut state = OrchestratorState::new(30_000, 10);
        let issue = make_issue("1", "Todo");
        // Activity was 100ms ago, timeout is 300000ms
        let entry = make_running_entry_with_stale_activity(issue, 100);
        state.running.insert("1".to_string(), entry);

        let result = reconcile_stalled_runs(&mut state, 300_000);
        assert!(result.is_empty());
        // Entry should still be running (not cancelled)
        assert!(state.running.get("1").unwrap().cancel_sent_at.is_none());
    }

    #[tokio::test]
    async fn test_stall_detected_sends_cancel_signal() {
        let mut state = OrchestratorState::new(30_000, 10);
        let issue = make_issue("1", "Todo");
        // Activity was 400000ms ago, timeout is 300000ms
        let entry = make_running_entry_with_stale_activity(issue, 400_000);
        state.running.insert("1".to_string(), entry);

        let result = reconcile_stalled_runs(&mut state, 300_000);
        // No force-kill yet (first detection just sends cancel)
        assert!(result.is_empty());
        // But cancel_sent_at should be set
        assert!(state.running.get("1").unwrap().cancel_sent_at.is_some());
    }

    #[tokio::test]
    async fn test_already_cancelled_not_re_checked_for_stall() {
        let mut state = OrchestratorState::new(30_000, 10);
        let issue = make_issue("1", "Todo");
        // Already cancelled 10s ago (under 30s hard deadline)
        let entry = make_running_entry_cancelled(issue, 10_000);
        state.running.insert("1".to_string(), entry);

        let result = reconcile_stalled_runs(&mut state, 300_000);
        // Should not force-kill (only 10s since cancel, hard deadline is 30s)
        assert!(result.is_empty());
        assert!(state.running.contains_key("1"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Two-Phase Cancel Tests (Hard Kill)
// ═══════════════════════════════════════════════════════════════════════════════

mod two_phase_cancel {
    use super::*;

    #[tokio::test]
    async fn test_force_kill_after_hard_deadline() {
        let mut state = OrchestratorState::new(30_000, 10);
        let issue = make_issue("1", "Todo");
        // Cancelled 35s ago (exceeds 30s hard deadline)
        let entry = make_running_entry_cancelled(issue, 35_000);
        state.running.insert("1".to_string(), entry);

        let result = reconcile_stalled_runs(&mut state, 300_000);
        // Should force-kill
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].issue_id, "1");
        assert_eq!(result[0].identifier, "TEST-1");
        assert_eq!(result[0].attempt, 2); // retry_attempt was Some(2)
                                          // Entry should be removed from running
        assert!(!state.running.contains_key("1"));
    }

    #[tokio::test]
    async fn test_no_force_kill_before_hard_deadline() {
        let mut state = OrchestratorState::new(30_000, 10);
        let issue = make_issue("1", "Todo");
        // Cancelled 20s ago (under 30s hard deadline)
        let entry = make_running_entry_cancelled(issue, 20_000);
        state.running.insert("1".to_string(), entry);

        let result = reconcile_stalled_runs(&mut state, 300_000);
        assert!(result.is_empty());
        assert!(state.running.contains_key("1"));
    }

    #[tokio::test]
    async fn test_multiple_entries_mixed_states() {
        let mut state = OrchestratorState::new(30_000, 10);

        // Entry 1: stale but not yet cancelled
        let issue1 = make_issue("1", "Todo");
        let entry1 = make_running_entry_with_stale_activity(issue1, 400_000);
        state.running.insert("1".to_string(), entry1);

        // Entry 2: cancelled and past hard deadline
        let issue2 = make_issue("2", "Todo");
        let entry2 = make_running_entry_cancelled(issue2, 35_000);
        state.running.insert("2".to_string(), entry2);

        // Entry 3: healthy (recent activity)
        let issue3 = make_issue("3", "Todo");
        let entry3 = make_running_entry_with_stale_activity(issue3, 100);
        state.running.insert("3".to_string(), entry3);

        let result = reconcile_stalled_runs(&mut state, 300_000);

        // Only entry 2 should be force-killed
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].issue_id, "2");

        // Entry 1 should now have cancel_sent_at set
        assert!(state.running.get("1").unwrap().cancel_sent_at.is_some());
        // Entry 3 should be unchanged
        assert!(state.running.get("3").unwrap().cancel_sent_at.is_none());
        // Entry 2 should be removed
        assert!(!state.running.contains_key("2"));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Part B: Reconcile Actions Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod reconcile_actions {
    use super::*;

    #[test]
    fn test_terminal_state_produces_terminate_and_clean() {
        let running = vec!["1".to_string()];
        let refreshed = vec![("1".to_string(), "Done".to_string())];
        let active = vec!["Todo".to_string(), "In Progress".to_string()];
        let terminal = vec!["Done".to_string(), "Closed".to_string()];
        let identifiers = vec![("1".to_string(), "TEST-1".to_string())];

        let actions =
            determine_reconcile_actions(&running, &refreshed, &active, &terminal, &identifiers);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ReconcileAction::TerminateAndClean {
                issue_id,
                identifier,
            } => {
                assert_eq!(issue_id, "1");
                assert_eq!(identifier, "TEST-1");
            }
            _ => panic!("Expected TerminateAndClean"),
        }
    }

    #[test]
    fn test_active_state_produces_update_snapshot() {
        let running = vec!["1".to_string()];
        let refreshed = vec![("1".to_string(), "In Progress".to_string())];
        let active = vec!["Todo".to_string(), "In Progress".to_string()];
        let terminal = vec!["Done".to_string(), "Closed".to_string()];
        let identifiers = vec![("1".to_string(), "TEST-1".to_string())];

        let actions =
            determine_reconcile_actions(&running, &refreshed, &active, &terminal, &identifiers);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ReconcileAction::UpdateSnapshot { issue_id } => {
                assert_eq!(issue_id, "1");
            }
            _ => panic!("Expected UpdateSnapshot"),
        }
    }

    #[test]
    fn test_other_state_produces_terminate_no_clean() {
        let running = vec!["1".to_string()];
        let refreshed = vec![("1".to_string(), "Backlog".to_string())];
        let active = vec!["Todo".to_string(), "In Progress".to_string()];
        let terminal = vec!["Done".to_string(), "Closed".to_string()];
        let identifiers = vec![("1".to_string(), "TEST-1".to_string())];

        let actions =
            determine_reconcile_actions(&running, &refreshed, &active, &terminal, &identifiers);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ReconcileAction::TerminateNoClean {
                issue_id,
                identifier,
            } => {
                assert_eq!(issue_id, "1");
                assert_eq!(identifier, "TEST-1");
            }
            _ => panic!("Expected TerminateNoClean"),
        }
    }

    #[test]
    fn test_non_running_issue_ignored() {
        let running = vec!["1".to_string()];
        // Issue "2" is not in running
        let refreshed = vec![("2".to_string(), "Done".to_string())];
        let active = vec!["Todo".to_string()];
        let terminal = vec!["Done".to_string()];
        let identifiers = vec![("2".to_string(), "TEST-2".to_string())];

        let actions =
            determine_reconcile_actions(&running, &refreshed, &active, &terminal, &identifiers);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_multiple_issues_mixed_actions() {
        let running = vec!["1".to_string(), "2".to_string(), "3".to_string()];
        let refreshed = vec![
            ("1".to_string(), "Done".to_string()),        // terminal
            ("2".to_string(), "In Progress".to_string()), // active
            ("3".to_string(), "Backlog".to_string()),     // other
        ];
        let active = vec!["Todo".to_string(), "In Progress".to_string()];
        let terminal = vec!["Done".to_string(), "Closed".to_string()];
        let identifiers = vec![
            ("1".to_string(), "TEST-1".to_string()),
            ("2".to_string(), "TEST-2".to_string()),
            ("3".to_string(), "TEST-3".to_string()),
        ];

        let actions =
            determine_reconcile_actions(&running, &refreshed, &active, &terminal, &identifiers);
        assert_eq!(actions.len(), 3);

        // Verify each action type
        let terminate_clean: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, ReconcileAction::TerminateAndClean { .. }))
            .collect();
        let update_snapshot: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, ReconcileAction::UpdateSnapshot { .. }))
            .collect();
        let terminate_no_clean: Vec<_> = actions
            .iter()
            .filter(|a| matches!(a, ReconcileAction::TerminateNoClean { .. }))
            .collect();

        assert_eq!(terminate_clean.len(), 1);
        assert_eq!(update_snapshot.len(), 1);
        assert_eq!(terminate_no_clean.len(), 1);
    }

    #[test]
    fn test_case_insensitive_state_matching() {
        let running = vec!["1".to_string()];
        let refreshed = vec![("1".to_string(), "done".to_string())]; // lowercase
        let active = vec!["Todo".to_string()];
        let terminal = vec!["Done".to_string()]; // capitalized
        let identifiers = vec![("1".to_string(), "TEST-1".to_string())];

        let actions =
            determine_reconcile_actions(&running, &refreshed, &active, &terminal, &identifiers);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            &actions[0],
            ReconcileAction::TerminateAndClean { .. }
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Terminate Running Entry Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod terminate_entry {
    use super::*;

    #[tokio::test]
    async fn test_terminate_removes_from_running_and_claimed() {
        let mut state = OrchestratorState::new(30_000, 10);
        let issue = make_issue("1", "Todo");
        let entry = make_running_entry_with_stale_activity(issue, 100);
        state.running.insert("1".to_string(), entry);
        state.claimed.insert("1".to_string());

        let result = terminate_running_entry(&mut state, "1");
        assert!(result.is_some());
        let (identifier, attempt) = result.unwrap();
        assert_eq!(identifier, "TEST-1");
        assert_eq!(attempt, 0);

        assert!(!state.running.contains_key("1"));
        assert!(!state.claimed.contains("1"));
    }

    #[tokio::test]
    async fn test_terminate_nonexistent_returns_none() {
        let mut state = OrchestratorState::new(30_000, 10);
        let result = terminate_running_entry(&mut state, "nonexistent");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_terminate_updates_runtime_totals() {
        let mut state = OrchestratorState::new(30_000, 10);
        let issue = make_issue("1", "Todo");
        let entry = make_running_entry_with_stale_activity(issue, 100);
        state.running.insert("1".to_string(), entry);
        state.claimed.insert("1".to_string());

        let initial_runtime = state.codex_totals.seconds_running_ms;
        terminate_running_entry(&mut state, "1");
        // Runtime should have increased
        assert!(state.codex_totals.seconds_running_ms >= initial_runtime);
    }
}
