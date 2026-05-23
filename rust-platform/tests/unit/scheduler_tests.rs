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

//! Unit tests for scheduler dispatch rules.
//!
//! Tests cover:
//! - All 7 dispatch eligibility rules individually
//! - Priority sorting (priority ASC -> created_at ASC -> identifier ASC)
//! - Blocker checking (non-terminal blockers prevent dispatch)
//! - Global concurrency limit
//! - Per-state concurrency limit
//! - Claimed issue exclusion
//! - Running issue exclusion
//! - State normalization

use std::collections::HashMap;
use std::time::Instant;

use chrono::{TimeZone, Utc};
use tokio_util::sync::CancellationToken;

use symphony_platform::models::{BlockerRef, Issue, LiveSession, OrchestratorState, RunningEntry};
use symphony_platform::orchestrator::scheduler::{
    is_active_state, is_terminal_state, should_dispatch, sort_for_dispatch, DispatchConfig,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

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

fn make_issue_with_time(
    id: &str,
    state: &str,
    priority: Option<i32>,
    year: i32,
    month: u32,
    day: u32,
) -> Issue {
    let mut issue = make_issue(id, state, priority);
    issue.created_at = Some(Utc.with_ymd_and_hms(year, month, day, 0, 0, 0).unwrap());
    issue
}

fn default_config() -> DispatchConfig {
    DispatchConfig::default()
}

fn make_running_entry(issue: Issue) -> RunningEntry {
    RunningEntry {
        worker_handle: tokio::spawn(async {}),
        cancel_token: CancellationToken::new(),
        identifier: issue.identifier.clone(),
        issue,
        session: LiveSession::new("thread-1".to_string(), "turn-1".to_string()),
        retry_attempt: None,
        started_at: Instant::now(),
        started_at_utc: Utc::now(),
        cancel_sent_at: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Rule 1: Required Fields Check
// ═══════════════════════════════════════════════════════════════════════════════

mod rule_required_fields {
    use super::*;

    #[test]
    fn test_dispatch_rejected_when_id_empty() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let mut issue = make_issue("1", "Todo", Some(1));
        issue.id = String::new();
        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_dispatch_rejected_when_identifier_empty() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let mut issue = make_issue("1", "Todo", Some(1));
        issue.identifier = String::new();
        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_dispatch_rejected_when_title_empty() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let mut issue = make_issue("1", "Todo", Some(1));
        issue.title = String::new();
        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_dispatch_rejected_when_state_empty() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let issue = make_issue("1", "", Some(1));
        assert!(!should_dispatch(&issue, &state, &config));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Rule 2: Active State Check
// ═══════════════════════════════════════════════════════════════════════════════

mod rule_active_state {
    use super::*;

    #[test]
    fn test_dispatch_allowed_for_active_state() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let issue = make_issue("1", "Todo", Some(1));
        assert!(should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_dispatch_allowed_for_in_progress_state() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let issue = make_issue("1", "In Progress", Some(1));
        assert!(should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_dispatch_rejected_for_non_active_state() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let issue = make_issue("1", "Backlog", Some(1));
        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_dispatch_rejected_for_terminal_state() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let issue = make_issue("1", "Done", Some(1));
        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_state_comparison_is_case_insensitive() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        // "todo" should match "Todo" in active_states
        let issue = make_issue("1", "todo", Some(1));
        assert!(should_dispatch(&issue, &state, &config));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Rule 3: Running Issue Exclusion
// ═══════════════════════════════════════════════════════════════════════════════

mod rule_running_exclusion {
    use super::*;

    #[tokio::test]
    async fn test_dispatch_rejected_when_already_running() {
        let mut state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let issue = make_issue("1", "Todo", Some(1));

        // Insert into running map
        let entry = make_running_entry(issue.clone());
        state.running.insert("1".to_string(), entry);

        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[tokio::test]
    async fn test_dispatch_allowed_when_different_issue_running() {
        let mut state = OrchestratorState::new(30_000, 10);
        let config = default_config();

        let running_issue = make_issue("2", "Todo", Some(1));
        let entry = make_running_entry(running_issue);
        state.running.insert("2".to_string(), entry);

        let candidate = make_issue("1", "Todo", Some(1));
        assert!(should_dispatch(&candidate, &state, &config));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Rule 4: Claimed Issue Exclusion
// ═══════════════════════════════════════════════════════════════════════════════

mod rule_claimed_exclusion {
    use super::*;

    #[test]
    fn test_dispatch_rejected_when_claimed() {
        let mut state = OrchestratorState::new(30_000, 10);
        state.claimed.insert("1".to_string());
        let config = default_config();
        let issue = make_issue("1", "Todo", Some(1));
        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_dispatch_allowed_when_not_claimed() {
        let mut state = OrchestratorState::new(30_000, 10);
        state.claimed.insert("2".to_string()); // Different issue claimed
        let config = default_config();
        let issue = make_issue("1", "Todo", Some(1));
        assert!(should_dispatch(&issue, &state, &config));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Rule 5: Global Concurrency Limit
// ═══════════════════════════════════════════════════════════════════════════════

mod rule_global_concurrency {
    use super::*;

    #[test]
    fn test_dispatch_rejected_when_no_global_slots() {
        let state = OrchestratorState::new(30_000, 0); // 0 max concurrent
        let config = DispatchConfig {
            max_concurrent_agents: 0,
            ..default_config()
        };
        let issue = make_issue("1", "Todo", Some(1));
        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[tokio::test]
    async fn test_dispatch_rejected_when_global_slots_full() {
        let mut state = OrchestratorState::new(30_000, 1); // max 1
        let config = DispatchConfig {
            max_concurrent_agents: 1,
            ..default_config()
        };

        // Fill the single slot
        let running_issue = make_issue("2", "Todo", Some(1));
        let entry = make_running_entry(running_issue);
        state.running.insert("2".to_string(), entry);

        let candidate = make_issue("1", "Todo", Some(1));
        assert!(!should_dispatch(&candidate, &state, &config));
    }

    #[test]
    fn test_dispatch_allowed_when_global_slots_available() {
        let state = OrchestratorState::new(30_000, 5);
        let config = DispatchConfig {
            max_concurrent_agents: 5,
            ..default_config()
        };
        let issue = make_issue("1", "Todo", Some(1));
        assert!(should_dispatch(&issue, &state, &config));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Rule 6: Per-State Concurrency Limit
// ═══════════════════════════════════════════════════════════════════════════════

mod rule_per_state_concurrency {
    use super::*;

    #[tokio::test]
    async fn test_dispatch_rejected_when_per_state_slots_full() {
        let mut state = OrchestratorState::new(30_000, 10);
        let mut by_state = HashMap::new();
        by_state.insert("todo".to_string(), 1); // Only 1 slot for "todo"
        let config = DispatchConfig {
            max_concurrent_agents_by_state: by_state,
            ..default_config()
        };

        // Fill the per-state slot
        let running_issue = make_issue("2", "Todo", Some(1));
        let entry = make_running_entry(running_issue);
        state.running.insert("2".to_string(), entry);

        let candidate = make_issue("1", "Todo", Some(1));
        assert!(!should_dispatch(&candidate, &state, &config));
    }

    #[tokio::test]
    async fn test_dispatch_allowed_when_different_state_slot_full() {
        let mut state = OrchestratorState::new(30_000, 10);
        let mut by_state = HashMap::new();
        by_state.insert("todo".to_string(), 1);
        by_state.insert("in progress".to_string(), 1);
        let config = DispatchConfig {
            max_concurrent_agents_by_state: by_state,
            ..default_config()
        };

        // Fill the "todo" slot
        let running_issue = make_issue("2", "Todo", Some(1));
        let entry = make_running_entry(running_issue);
        state.running.insert("2".to_string(), entry);

        // "In Progress" slot is still available
        let candidate = make_issue("1", "In Progress", Some(1));
        assert!(should_dispatch(&candidate, &state, &config));
    }

    #[test]
    fn test_dispatch_allowed_when_no_per_state_limit_configured() {
        let state = OrchestratorState::new(30_000, 10);
        let config = DispatchConfig {
            max_concurrent_agents_by_state: HashMap::new(), // No per-state limits
            ..default_config()
        };
        let issue = make_issue("1", "Todo", Some(1));
        assert!(should_dispatch(&issue, &state, &config));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Rule 7: Blocker Check
// ═══════════════════════════════════════════════════════════════════════════════

mod rule_blocker_check {
    use super::*;

    #[test]
    fn test_dispatch_rejected_when_blocked_by_non_terminal_issue() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let mut issue = make_issue("1", "Todo", Some(1));
        issue.blocked_by = vec![BlockerRef {
            id: Some("2".to_string()),
            identifier: Some("TEST-2".to_string()),
            state: Some("In Progress".to_string()), // non-terminal
        }];
        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_dispatch_allowed_when_blocker_is_terminal() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let mut issue = make_issue("1", "Todo", Some(1));
        issue.blocked_by = vec![BlockerRef {
            id: Some("2".to_string()),
            identifier: Some("TEST-2".to_string()),
            state: Some("Done".to_string()), // terminal
        }];
        assert!(should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_dispatch_rejected_when_blocker_state_unknown() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let mut issue = make_issue("1", "Todo", Some(1));
        issue.blocked_by = vec![BlockerRef {
            id: Some("2".to_string()),
            identifier: Some("TEST-2".to_string()),
            state: None, // Unknown state treated as non-terminal
        }];
        assert!(!should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_dispatch_allowed_when_blocked_but_not_in_blocker_check_state() {
        let state = OrchestratorState::new(30_000, 10);
        let config = DispatchConfig {
            blocker_check_states: vec!["todo".to_string()], // Only check blockers for "todo"
            ..default_config()
        };
        let mut issue = make_issue("1", "In Progress", Some(1));
        issue.blocked_by = vec![BlockerRef {
            id: Some("2".to_string()),
            identifier: Some("TEST-2".to_string()),
            state: Some("In Progress".to_string()),
        }];
        // "In Progress" is not in blocker_check_states, so blockers are ignored
        assert!(should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_dispatch_allowed_when_all_blockers_terminal() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let mut issue = make_issue("1", "Todo", Some(1));
        issue.blocked_by = vec![
            BlockerRef {
                id: Some("2".to_string()),
                identifier: Some("TEST-2".to_string()),
                state: Some("Done".to_string()),
            },
            BlockerRef {
                id: Some("3".to_string()),
                identifier: Some("TEST-3".to_string()),
                state: Some("Closed".to_string()),
            },
        ];
        assert!(should_dispatch(&issue, &state, &config));
    }

    #[test]
    fn test_dispatch_rejected_when_any_blocker_non_terminal() {
        let state = OrchestratorState::new(30_000, 10);
        let config = default_config();
        let mut issue = make_issue("1", "Todo", Some(1));
        issue.blocked_by = vec![
            BlockerRef {
                id: Some("2".to_string()),
                identifier: Some("TEST-2".to_string()),
                state: Some("Done".to_string()), // terminal
            },
            BlockerRef {
                id: Some("3".to_string()),
                identifier: Some("TEST-3".to_string()),
                state: Some("In Progress".to_string()), // non-terminal
            },
        ];
        assert!(!should_dispatch(&issue, &state, &config));
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Sort for Dispatch Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod sort_dispatch {
    use super::*;

    #[test]
    fn test_sort_by_priority_ascending() {
        let mut issues = vec![
            make_issue("3", "Todo", Some(3)),
            make_issue("1", "Todo", Some(1)),
            make_issue("2", "Todo", Some(2)),
        ];

        sort_for_dispatch(&mut issues);

        assert_eq!(issues[0].id, "1"); // priority 1
        assert_eq!(issues[1].id, "2"); // priority 2
        assert_eq!(issues[2].id, "3"); // priority 3
    }

    #[test]
    fn test_sort_null_priority_last() {
        let mut issues = vec![
            make_issue("1", "Todo", None),
            make_issue("2", "Todo", Some(1)),
            make_issue("3", "Todo", None),
            make_issue("4", "Todo", Some(5)),
        ];

        sort_for_dispatch(&mut issues);

        assert_eq!(issues[0].id, "2"); // priority 1
        assert_eq!(issues[1].id, "4"); // priority 5
                                       // None priorities come last
        assert!(issues[2].priority.is_none());
        assert!(issues[3].priority.is_none());
    }

    #[test]
    fn test_sort_created_at_tiebreak_oldest_first() {
        let mut issues = vec![
            make_issue_with_time("1", "Todo", Some(1), 2024, 3, 1),
            make_issue_with_time("2", "Todo", Some(1), 2024, 1, 1),
            make_issue_with_time("3", "Todo", Some(1), 2024, 2, 1),
        ];

        sort_for_dispatch(&mut issues);

        assert_eq!(issues[0].id, "2"); // Jan 1 (oldest)
        assert_eq!(issues[1].id, "3"); // Feb 1
        assert_eq!(issues[2].id, "1"); // Mar 1 (newest)
    }

    #[test]
    fn test_sort_identifier_tiebreak_lexicographic() {
        let now = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let mut issues = vec![
            {
                let mut i = make_issue("3", "Todo", Some(1));
                i.identifier = "TEST-30".to_string();
                i.created_at = Some(now);
                i
            },
            {
                let mut i = make_issue("1", "Todo", Some(1));
                i.identifier = "TEST-10".to_string();
                i.created_at = Some(now);
                i
            },
            {
                let mut i = make_issue("2", "Todo", Some(1));
                i.identifier = "TEST-20".to_string();
                i.created_at = Some(now);
                i
            },
        ];

        sort_for_dispatch(&mut issues);

        assert_eq!(issues[0].identifier, "TEST-10");
        assert_eq!(issues[1].identifier, "TEST-20");
        assert_eq!(issues[2].identifier, "TEST-30");
    }

    #[test]
    fn test_sort_empty_list() {
        let mut issues: Vec<Issue> = vec![];
        sort_for_dispatch(&mut issues);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_sort_single_issue() {
        let mut issues = vec![make_issue("1", "Todo", Some(1))];
        sort_for_dispatch(&mut issues);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].id, "1");
    }

    #[test]
    fn test_sort_combined_priority_and_time() {
        let mut issues = vec![
            make_issue_with_time("1", "Todo", Some(2), 2024, 1, 1), // pri 2, oldest
            make_issue_with_time("2", "Todo", Some(1), 2024, 3, 1), // pri 1, newest
            make_issue_with_time("3", "Todo", Some(1), 2024, 2, 1), // pri 1, middle
        ];

        sort_for_dispatch(&mut issues);

        // Priority 1 issues first, sorted by created_at
        assert_eq!(issues[0].id, "3"); // pri 1, Feb
        assert_eq!(issues[1].id, "2"); // pri 1, Mar
        assert_eq!(issues[2].id, "1"); // pri 2
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// State Helper Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod state_helpers {
    use super::*;

    #[test]
    fn test_is_terminal_state_match() {
        let terminals = vec![
            "Done".to_string(),
            "Closed".to_string(),
            "Cancelled".to_string(),
        ];
        assert!(is_terminal_state("Done", &terminals));
        assert!(is_terminal_state("done", &terminals)); // case insensitive
        assert!(is_terminal_state("DONE", &terminals));
        assert!(is_terminal_state("Closed", &terminals));
    }

    #[test]
    fn test_is_terminal_state_no_match() {
        let terminals = vec!["Done".to_string(), "Closed".to_string()];
        assert!(!is_terminal_state("In Progress", &terminals));
        assert!(!is_terminal_state("Todo", &terminals));
        assert!(!is_terminal_state("Backlog", &terminals));
    }

    #[test]
    fn test_is_active_state_match() {
        let active = vec!["Todo".to_string(), "In Progress".to_string()];
        assert!(is_active_state("Todo", &active));
        assert!(is_active_state("todo", &active)); // case insensitive
        assert!(is_active_state("In Progress", &active));
    }

    #[test]
    fn test_is_active_state_no_match() {
        let active = vec!["Todo".to_string(), "In Progress".to_string()];
        assert!(!is_active_state("Done", &active));
        assert!(!is_active_state("Backlog", &active));
    }
}
