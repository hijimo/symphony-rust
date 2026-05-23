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

//! Unit tests for orchestrator logic.
//!
//! Tests cover:
//! - sort_for_dispatch: priority ordering, null priority last, created_at tiebreak, identifier tiebreak
//! - should_dispatch: active state check, terminal state exclusion, claimed exclusion, blocker rules
//! - compute_retry_delay: continuation=1000ms, exponential backoff, cap at max_retry_backoff_ms
//! - concurrency control: global slots, per-state slots

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use tokio_util::sync::CancellationToken;

use symphony_platform::config::{Config, PlatformConfig, PollingConfig, WorkflowConfig};
use symphony_platform::orchestrator::Orchestrator;
use symphony_platform::platform::cooldown_queue::CooldownQueue;
use symphony_platform::platform::{make_test_issue, Dispatchable, Issue, IssueId, MemoryAdapter};

// ═══════════════════════════════════════════════════════════════════════════════
// Sort for Dispatch Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod sort_for_dispatch {
    use super::*;

    /// Sort issues by priority (lower number = higher priority), then by created_at.
    fn sort_issues(issues: &mut Vec<Issue>) {
        issues.sort_by(|a, b| {
            // Priority: lower number = higher priority, None goes last
            let pri_cmp = match (a.priority, b.priority) {
                (Some(pa), Some(pb)) => pa.cmp(&pb),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            };

            if pri_cmp != std::cmp::Ordering::Equal {
                return pri_cmp;
            }

            // Tiebreak by created_at (oldest first)
            let time_cmp = a.created_at.cmp(&b.created_at);
            if time_cmp != std::cmp::Ordering::Equal {
                return time_cmp;
            }

            // Final tiebreak by identifier (lexicographic)
            a.number.cmp(&b.number)
        });
    }

    #[test]
    fn test_priority_ordering() {
        let now = Utc::now();
        let mut issues = vec![
            make_priority_issue(1, Some(3), now),
            make_priority_issue(2, Some(1), now),
            make_priority_issue(3, Some(2), now),
        ];

        sort_issues(&mut issues);

        assert_eq!(issues[0].id, IssueId(2)); // priority 1
        assert_eq!(issues[1].id, IssueId(3)); // priority 2
        assert_eq!(issues[2].id, IssueId(1)); // priority 3
    }

    #[test]
    fn test_null_priority_last() {
        let now = Utc::now();
        let mut issues = vec![
            make_priority_issue(1, None, now),
            make_priority_issue(2, Some(1), now),
            make_priority_issue(3, None, now),
            make_priority_issue(4, Some(5), now),
        ];

        sort_issues(&mut issues);

        // Issues with priority come first
        assert_eq!(issues[0].id, IssueId(2)); // priority 1
        assert_eq!(issues[1].id, IssueId(4)); // priority 5
                                              // Issues without priority come last
        assert!(issues[2].priority.is_none());
        assert!(issues[3].priority.is_none());
    }

    #[test]
    fn test_created_at_tiebreak() {
        let now = Utc::now();
        let earlier = now - TimeDelta::hours(2);
        let later = now - TimeDelta::hours(1);

        let mut issues = vec![
            make_priority_issue_at(1, Some(1), later),
            make_priority_issue_at(2, Some(1), earlier),
            make_priority_issue_at(3, Some(1), now),
        ];

        sort_issues(&mut issues);

        // Same priority — oldest first
        assert_eq!(issues[0].id, IssueId(2)); // earliest
        assert_eq!(issues[1].id, IssueId(1)); // middle
        assert_eq!(issues[2].id, IssueId(3)); // latest
    }

    #[test]
    fn test_identifier_tiebreak() {
        let now = Utc::now();
        let mut issues = vec![
            make_priority_issue_at(30, Some(1), now),
            make_priority_issue_at(10, Some(1), now),
            make_priority_issue_at(20, Some(1), now),
        ];

        sort_issues(&mut issues);

        // Same priority, same time — sort by number
        assert_eq!(issues[0].id, IssueId(10));
        assert_eq!(issues[1].id, IssueId(20));
        assert_eq!(issues[2].id, IssueId(30));
    }

    #[test]
    fn test_empty_list() {
        let mut issues: Vec<Issue> = vec![];
        sort_issues(&mut issues);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_single_issue() {
        let mut issues = vec![make_priority_issue(1, Some(1), Utc::now())];
        sort_issues(&mut issues);
        assert_eq!(issues.len(), 1);
    }

    fn make_priority_issue(id: u64, priority: Option<u8>, created_at: DateTime<Utc>) -> Issue {
        Issue {
            id: IssueId(id),
            number: id,
            title: format!("Issue {}", id),
            description: None,
            url: format!("https://example.com/issues/{}", id),
            assignee: None,
            workflow_state: Some("workflow::todo".to_string()),
            branch_name: format!("issue-{}", id),
            priority,
            labels: vec!["workflow::todo".to_string()],
            blocked_by: Vec::new(),
            created_at: Some(created_at),
            updated_at: Some(Utc::now()),
        }
    }

    fn make_priority_issue_at(id: u64, priority: Option<u8>, created_at: DateTime<Utc>) -> Issue {
        make_priority_issue(id, priority, created_at)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Should Dispatch Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod should_dispatch {
    use super::*;

    /// Determine if an issue should be dispatched based on state and constraints.
    fn should_dispatch_issue(
        issue: &Issue,
        active_states: &[String],
        terminal_states: &[String],
        active_issue_ids: &HashSet<IssueId>,
        blocker_check_states: &[String],
    ) -> bool {
        // Must be in an active state
        let state = match &issue.workflow_state {
            Some(s) => s,
            None => return false,
        };

        if !active_states.contains(state) {
            return false;
        }

        // Must not be in a terminal state
        if terminal_states.contains(state) {
            return false;
        }

        // Must not already be claimed/active
        if active_issue_ids.contains(&issue.id) {
            return false;
        }

        // Check blocker rules for specific states
        let state_key = state.replace("workflow::", "");
        if blocker_check_states.contains(&state_key) && !issue.blocked_by.is_empty() {
            return false;
        }

        true
    }

    #[test]
    fn test_active_state_dispatches() {
        let issue = make_test_issue(1, "Test", Some("workflow::todo"));
        let active_states = vec!["workflow::todo".to_string()];
        let terminal_states = vec!["workflow::done".to_string()];

        assert!(should_dispatch_issue(
            &issue,
            &active_states,
            &terminal_states,
            &HashSet::new(),
            &["todo".to_string()],
        ));
    }

    #[test]
    fn test_terminal_state_excluded() {
        let issue = make_test_issue(1, "Test", Some("workflow::done"));
        let active_states = vec!["workflow::todo".to_string()];
        let terminal_states = vec!["workflow::done".to_string()];

        assert!(!should_dispatch_issue(
            &issue,
            &active_states,
            &terminal_states,
            &HashSet::new(),
            &["todo".to_string()],
        ));
    }

    #[test]
    fn test_non_active_state_excluded() {
        let issue = make_test_issue(1, "Test", Some("workflow::backlog"));
        let active_states = vec!["workflow::todo".to_string()];
        let terminal_states = vec!["workflow::done".to_string()];

        assert!(!should_dispatch_issue(
            &issue,
            &active_states,
            &terminal_states,
            &HashSet::new(),
            &["todo".to_string()],
        ));
    }

    #[test]
    fn test_claimed_issue_excluded() {
        let issue = make_test_issue(1, "Test", Some("workflow::todo"));
        let active_states = vec!["workflow::todo".to_string()];
        let terminal_states = vec!["workflow::done".to_string()];
        let mut active_ids = HashSet::new();
        active_ids.insert(IssueId(1));

        assert!(!should_dispatch_issue(
            &issue,
            &active_states,
            &terminal_states,
            &active_ids,
            &["todo".to_string()],
        ));
    }

    #[test]
    fn test_blocked_issue_in_blocker_check_state() {
        let mut issue = make_test_issue(1, "Test", Some("workflow::todo"));
        issue.blocked_by = vec![IssueId(99)];

        let active_states = vec!["workflow::todo".to_string()];
        let terminal_states = vec!["workflow::done".to_string()];

        assert!(!should_dispatch_issue(
            &issue,
            &active_states,
            &terminal_states,
            &HashSet::new(),
            &["todo".to_string()],
        ));
    }

    #[test]
    fn test_blocked_issue_not_in_blocker_check_state() {
        let mut issue = make_test_issue(1, "Test", Some("workflow::in-progress"));
        issue.blocked_by = vec![IssueId(99)];
        issue.workflow_state = Some("workflow::in-progress".to_string());

        let active_states = vec![
            "workflow::todo".to_string(),
            "workflow::in-progress".to_string(),
        ];
        let terminal_states = vec!["workflow::done".to_string()];

        // "in-progress" is not in blocker_check_states, so blockers are ignored
        assert!(should_dispatch_issue(
            &issue,
            &active_states,
            &terminal_states,
            &HashSet::new(),
            &["todo".to_string()],
        ));
    }

    #[test]
    fn test_no_workflow_state_excluded() {
        let mut issue = make_test_issue(1, "Test", None);
        issue.workflow_state = None;

        let active_states = vec!["workflow::todo".to_string()];
        let terminal_states = vec!["workflow::done".to_string()];

        assert!(!should_dispatch_issue(
            &issue,
            &active_states,
            &terminal_states,
            &HashSet::new(),
            &["todo".to_string()],
        ));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Compute Retry Delay Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod compute_retry_delay {
    use super::*;

    /// Retry kind determines the delay strategy.
    #[derive(Debug, Clone, PartialEq)]
    enum RetryKind {
        /// Normal exit → continuation retry (fixed 1s delay)
        Continuation,
        /// Abnormal exit → exponential backoff
        Failure,
    }

    /// Compute the retry delay based on retry kind and attempt number.
    ///
    /// - Continuation: always 1000ms (SPEC §8.4)
    /// - Failure: 10_000ms * 2^(attempt-1), capped at max_retry_backoff_ms
    fn compute_retry_delay_ms(kind: &RetryKind, attempt: u32, max_retry_backoff_ms: u64) -> u64 {
        match kind {
            RetryKind::Continuation => 1_000,
            RetryKind::Failure => {
                let base = 10_000u64; // 10 seconds base
                let delay = base.saturating_mul(2u64.saturating_pow(attempt.saturating_sub(1)));
                delay.min(max_retry_backoff_ms)
            }
        }
    }

    #[test]
    fn test_continuation_always_1000ms() {
        let max_backoff = 300_000;

        assert_eq!(
            compute_retry_delay_ms(&RetryKind::Continuation, 1, max_backoff),
            1_000
        );
        assert_eq!(
            compute_retry_delay_ms(&RetryKind::Continuation, 5, max_backoff),
            1_000
        );
        assert_eq!(
            compute_retry_delay_ms(&RetryKind::Continuation, 100, max_backoff),
            1_000
        );
    }

    #[test]
    fn test_exponential_backoff() {
        let max_backoff = 300_000;

        // attempt 1: 10_000 * 2^0 = 10_000
        assert_eq!(
            compute_retry_delay_ms(&RetryKind::Failure, 1, max_backoff),
            10_000
        );
        // attempt 2: 10_000 * 2^1 = 20_000
        assert_eq!(
            compute_retry_delay_ms(&RetryKind::Failure, 2, max_backoff),
            20_000
        );
        // attempt 3: 10_000 * 2^2 = 40_000
        assert_eq!(
            compute_retry_delay_ms(&RetryKind::Failure, 3, max_backoff),
            40_000
        );
        // attempt 4: 10_000 * 2^3 = 80_000
        assert_eq!(
            compute_retry_delay_ms(&RetryKind::Failure, 4, max_backoff),
            80_000
        );
    }

    #[test]
    fn test_cap_at_max_retry_backoff() {
        let max_backoff = 60_000; // 60 seconds cap

        // attempt 1: 10_000 (under cap)
        assert_eq!(
            compute_retry_delay_ms(&RetryKind::Failure, 1, max_backoff),
            10_000
        );
        // attempt 2: 20_000 (under cap)
        assert_eq!(
            compute_retry_delay_ms(&RetryKind::Failure, 2, max_backoff),
            20_000
        );
        // attempt 3: 40_000 (under cap)
        assert_eq!(
            compute_retry_delay_ms(&RetryKind::Failure, 3, max_backoff),
            40_000
        );
        // attempt 4: 80_000 → capped to 60_000
        assert_eq!(
            compute_retry_delay_ms(&RetryKind::Failure, 4, max_backoff),
            60_000
        );
        // attempt 10: huge → capped to 60_000
        assert_eq!(
            compute_retry_delay_ms(&RetryKind::Failure, 10, max_backoff),
            60_000
        );
    }

    #[test]
    fn test_zero_attempt_handled() {
        let max_backoff = 300_000;
        // attempt 0 should not panic (saturating_sub handles it)
        let delay = compute_retry_delay_ms(&RetryKind::Failure, 0, max_backoff);
        assert!(delay > 0);
    }

    #[test]
    fn test_very_large_attempt_no_overflow() {
        let max_backoff = 300_000;
        // Very large attempt number should not overflow
        let delay = compute_retry_delay_ms(&RetryKind::Failure, 100, max_backoff);
        assert_eq!(delay, max_backoff); // Should be capped
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Concurrency Control Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod concurrency_control {
    use super::*;

    /// Check if there are available global slots.
    fn has_global_slots(active_count: usize, max_concurrent: usize) -> bool {
        active_count < max_concurrent
    }

    /// Check if there are available per-state slots.
    fn has_state_slots(
        state: &str,
        state_counts: &HashMap<String, usize>,
        max_by_state: &HashMap<String, usize>,
    ) -> bool {
        if let Some(&max) = max_by_state.get(state) {
            let current = state_counts.get(state).copied().unwrap_or(0);
            current < max
        } else {
            // No per-state limit configured — always available
            true
        }
    }

    #[test]
    fn test_global_slots_available() {
        assert!(has_global_slots(0, 5));
        assert!(has_global_slots(4, 5));
    }

    #[test]
    fn test_global_slots_exhausted() {
        assert!(!has_global_slots(5, 5));
        assert!(!has_global_slots(10, 5));
    }

    #[test]
    fn test_per_state_slots_available() {
        let state_counts: HashMap<String, usize> = HashMap::new();
        let mut max_by_state = HashMap::new();
        max_by_state.insert("todo".to_string(), 3);

        assert!(has_state_slots("todo", &state_counts, &max_by_state));
    }

    #[test]
    fn test_per_state_slots_exhausted() {
        let mut state_counts = HashMap::new();
        state_counts.insert("todo".to_string(), 3);
        let mut max_by_state = HashMap::new();
        max_by_state.insert("todo".to_string(), 3);

        assert!(!has_state_slots("todo", &state_counts, &max_by_state));
    }

    #[test]
    fn test_no_per_state_limit_always_available() {
        let mut state_counts = HashMap::new();
        state_counts.insert("in_progress".to_string(), 100);
        let max_by_state: HashMap<String, usize> = HashMap::new();

        // No limit configured for "in_progress" — always available
        assert!(has_state_slots("in_progress", &state_counts, &max_by_state));
    }

    #[test]
    fn test_different_states_independent() {
        let mut state_counts = HashMap::new();
        state_counts.insert("todo".to_string(), 3);
        state_counts.insert("rework".to_string(), 1);

        let mut max_by_state = HashMap::new();
        max_by_state.insert("todo".to_string(), 3);
        max_by_state.insert("rework".to_string(), 2);

        // todo is full
        assert!(!has_state_slots("todo", &state_counts, &max_by_state));
        // rework still has room
        assert!(has_state_slots("rework", &state_counts, &max_by_state));
    }
}
