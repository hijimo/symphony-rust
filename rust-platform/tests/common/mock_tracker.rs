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

//! MockTracker — configurable tracker mock for integration tests.
//!
//! Simulates the issue tracker (Linear/GitHub/GitLab) with configurable
//! candidate issues, state responses, and call recording for verification.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};

use symphony_platform::platform::{Issue, IssueId};

/// Records of calls made to the mock tracker.
#[derive(Debug, Clone, Default)]
pub struct TrackerCallLog {
    pub fetch_candidates_calls: Vec<FetchCandidatesCall>,
    pub fetch_state_calls: Vec<FetchStateCall>,
    pub set_state_calls: Vec<SetStateCall>,
}

#[derive(Debug, Clone)]
pub struct FetchCandidatesCall {
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct FetchStateCall {
    pub issue_ids: Vec<IssueId>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SetStateCall {
    pub issue_id: IssueId,
    pub new_state: String,
    pub timestamp: DateTime<Utc>,
}

/// A configurable mock tracker for testing orchestrator behavior.
///
/// Supports:
/// - Configurable candidate issues returned by fetch
/// - Configurable per-issue state responses
/// - Call recording for verification in assertions
/// - Dynamic state mutation (simulating external state changes)
pub struct MockTracker {
    /// Issues returned by fetch_candidates
    candidates: Arc<Mutex<Vec<Issue>>>,
    /// Per-issue state overrides (issue_id -> current state)
    state_overrides: Arc<Mutex<HashMap<u64, String>>>,
    /// Call log for verification
    call_log: Arc<Mutex<TrackerCallLog>>,
    /// If set, fetch_candidates will return this error
    fetch_error: Arc<Mutex<Option<String>>>,
}

impl MockTracker {
    /// Create a new MockTracker with the given candidate issues.
    pub fn new(candidates: Vec<Issue>) -> Self {
        Self {
            candidates: Arc::new(Mutex::new(candidates)),
            state_overrides: Arc::new(Mutex::new(HashMap::new())),
            call_log: Arc::new(Mutex::new(TrackerCallLog::default())),
            fetch_error: Arc::new(Mutex::new(None)),
        }
    }

    /// Create an empty MockTracker.
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Set the candidate issues that will be returned.
    pub fn set_candidates(&self, candidates: Vec<Issue>) {
        *self.candidates.lock().unwrap() = candidates;
    }

    /// Add a single candidate issue.
    pub fn add_candidate(&self, issue: Issue) {
        self.candidates.lock().unwrap().push(issue);
    }

    /// Override the state for a specific issue (simulates external state change).
    pub fn set_issue_state(&self, issue_id: u64, state: &str) {
        self.state_overrides
            .lock()
            .unwrap()
            .insert(issue_id, state.to_string());
    }

    /// Set an error to be returned on the next fetch_candidates call.
    pub fn set_fetch_error(&self, error: &str) {
        *self.fetch_error.lock().unwrap() = Some(error.to_string());
    }

    /// Clear any pending fetch error.
    pub fn clear_fetch_error(&self) {
        *self.fetch_error.lock().unwrap() = None;
    }

    /// Get the call log for verification.
    pub fn call_log(&self) -> TrackerCallLog {
        self.call_log.lock().unwrap().clone()
    }

    /// Get the number of fetch_candidates calls made.
    pub fn fetch_candidates_count(&self) -> usize {
        self.call_log.lock().unwrap().fetch_candidates_calls.len()
    }

    /// Get the number of fetch_state calls made.
    pub fn fetch_state_count(&self) -> usize {
        self.call_log.lock().unwrap().fetch_state_calls.len()
    }

    /// Simulate fetching candidate issues.
    pub fn fetch_candidates(&self) -> Result<Vec<Issue>, String> {
        // Record the call
        self.call_log
            .lock()
            .unwrap()
            .fetch_candidates_calls
            .push(FetchCandidatesCall {
                timestamp: Utc::now(),
            });

        // Check for configured error
        if let Some(err) = self.fetch_error.lock().unwrap().take() {
            return Err(err);
        }

        // Return candidates with any state overrides applied
        let candidates = self.candidates.lock().unwrap().clone();
        let overrides = self.state_overrides.lock().unwrap();

        let result: Vec<Issue> = candidates
            .into_iter()
            .map(|mut issue| {
                if let Some(new_state) = overrides.get(&issue.id.0) {
                    issue.workflow_state = Some(new_state.clone());
                    // Update labels to match
                    issue.labels.retain(|l| !l.starts_with("workflow::"));
                    issue.labels.push(new_state.clone());
                }
                issue
            })
            .collect();

        Ok(result)
    }

    /// Simulate fetching states for specific issue IDs.
    pub fn fetch_issue_states(&self, issue_ids: &[IssueId]) -> Result<Vec<Issue>, String> {
        // Record the call
        self.call_log
            .lock()
            .unwrap()
            .fetch_state_calls
            .push(FetchStateCall {
                issue_ids: issue_ids.to_vec(),
                timestamp: Utc::now(),
            });

        let candidates = self.candidates.lock().unwrap();
        let overrides = self.state_overrides.lock().unwrap();

        let result: Vec<Issue> = candidates
            .iter()
            .filter(|i| issue_ids.contains(&i.id))
            .cloned()
            .map(|mut issue| {
                if let Some(new_state) = overrides.get(&issue.id.0) {
                    issue.workflow_state = Some(new_state.clone());
                    issue.labels.retain(|l| !l.starts_with("workflow::"));
                    issue.labels.push(new_state.clone());
                }
                issue
            })
            .collect();

        Ok(result)
    }

    /// Simulate setting the state of an issue.
    pub fn set_workflow_state(&self, issue_id: IssueId, new_state: &str) -> Result<(), String> {
        self.call_log
            .lock()
            .unwrap()
            .set_state_calls
            .push(SetStateCall {
                issue_id,
                new_state: new_state.to_string(),
                timestamp: Utc::now(),
            });

        self.state_overrides
            .lock()
            .unwrap()
            .insert(issue_id.0, new_state.to_string());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::create_test_issue;

    #[test]
    fn test_mock_tracker_returns_candidates() {
        let issue = create_test_issue(1, "PROJ-1", "Test issue");
        let tracker = MockTracker::new(vec![issue.clone()]);

        let result = tracker.fetch_candidates().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, IssueId(1));
    }

    #[test]
    fn test_mock_tracker_state_override() {
        let issue = create_test_issue(1, "PROJ-1", "Test issue");
        let tracker = MockTracker::new(vec![issue]);

        tracker.set_issue_state(1, "workflow::done");

        let result = tracker.fetch_candidates().unwrap();
        assert_eq!(result[0].workflow_state, Some("workflow::done".to_string()));
    }

    #[test]
    fn test_mock_tracker_records_calls() {
        let tracker = MockTracker::new(vec![]);

        tracker.fetch_candidates().unwrap();
        tracker.fetch_candidates().unwrap();

        assert_eq!(tracker.fetch_candidates_count(), 2);
    }

    #[test]
    fn test_mock_tracker_fetch_error() {
        let tracker = MockTracker::new(vec![]);
        tracker.set_fetch_error("network timeout");

        let result = tracker.fetch_candidates();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "network timeout");

        // Error is consumed — next call succeeds
        let result = tracker.fetch_candidates();
        assert!(result.is_ok());
    }
}
