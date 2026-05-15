//! Workflow state machine for label-based issue state management.
//!
//! Implements the "add-then-remove" compensating transaction strategy to ensure
//! an issue never enters a "ghost" state (no workflow label). The state machine
//! operates on top of the `Platform` trait's label operations.
//!
//! Key design decisions:
//! - Each issue should have exactly ONE `workflow::*` label at any time.
//! - State transitions first add the new label, then remove old ones.
//! - A verification step ensures the final state is correct.
//! - If verification finds anomalies (multiple labels or zero labels), it
//!   performs corrective action.

use crate::config::WorkflowConfig;
use crate::error::PlatformError;
use crate::platform::{IssueId, Platform};

/// Manages workflow state transitions using label-based state representation.
///
/// The state machine ensures that:
/// 1. An issue always has exactly one workflow label (no ghost states).
/// 2. Transitions are safe even if partial failures occur.
/// 3. Stale labels are cleaned up after transitions.
///
/// # Concurrency
///
/// `set_workflow_state` should not be called concurrently for the same issue.
/// The Orchestrator guarantees this by being the single entry point for state
/// changes, executing them serially per issue.
pub struct WorkflowStateMachine {
    config: WorkflowConfig,
}

impl WorkflowStateMachine {
    /// Creates a new workflow state machine with the given configuration.
    pub fn new(config: WorkflowConfig) -> Self {
        Self { config }
    }

    /// Returns a reference to the underlying workflow configuration.
    pub fn config(&self) -> &WorkflowConfig {
        &self.config
    }

    /// Resolves an internal state key (e.g., "todo") to its label name
    /// (e.g., "workflow::todo").
    ///
    /// # Errors
    ///
    /// Returns `PlatformError::MissingState` if the state key is not defined
    /// in the workflow configuration.
    fn workflow_label(&self, state: &str) -> Result<String, PlatformError> {
        self.config
            .states
            .get(state)
            .cloned()
            .ok_or_else(|| PlatformError::MissingState(state.to_string()))
    }

    /// Returns all label values that represent workflow states.
    fn all_workflow_label_values(&self) -> Vec<String> {
        self.config.states.values().cloned().collect()
    }

    /// Gets the current workflow labels on an issue by fetching it and filtering.
    ///
    /// Returns only labels that match a known workflow state value.
    pub async fn get_current_workflow_labels(
        &self,
        platform: &dyn Platform,
        issue_id: IssueId,
    ) -> Result<Vec<String>, PlatformError> {
        let issue = platform.fetch_issue(issue_id).await?;
        let workflow_label_values = self.all_workflow_label_values();

        let workflow_labels: Vec<String> = issue
            .labels
            .into_iter()
            .filter(|l| workflow_label_values.contains(l))
            .collect();

        Ok(workflow_labels)
    }

    /// Transitions an issue to a new workflow state.
    ///
    /// Uses the "add-then-remove" strategy (compensating transaction):
    /// 1. Add the target workflow label (ensures issue always has at least one).
    /// 2. Remove all other workflow labels (cleanup stale state).
    /// 3. Verify the final state is correct (exactly one workflow label).
    ///
    /// # Arguments
    ///
    /// * `platform` — The platform adapter to use for label operations.
    /// * `issue_id` — The issue to transition.
    /// * `target_state` — The internal state key (e.g., "in_progress").
    ///
    /// # Errors
    ///
    /// - `PlatformError::MissingState` if `target_state` is not in the config.
    /// - Any platform error from the underlying label operations.
    /// - Partial label removal failures are logged but do not fail the operation
    ///   (the target label is already applied).
    pub async fn set_workflow_state(
        &self,
        platform: &dyn Platform,
        issue_id: IssueId,
        target_state: &str,
    ) -> Result<(), PlatformError> {
        let target_label = self.workflow_label(target_state)?;

        // Get current workflow labels on the issue
        let current_labels = self.get_current_workflow_labels(platform, issue_id).await?;

        // If the issue already has exactly the target label and nothing else, no-op
        if current_labels.len() == 1 && current_labels[0] == target_label {
            tracing::debug!(
                issue_id = %issue_id,
                state = target_state,
                "Issue already in target state, skipping"
            );
            return Ok(());
        }

        // Step 1: Add the new label first (ensures issue is never without a workflow label)
        platform
            .add_labels(issue_id, &[target_label.clone()])
            .await?;

        // Step 2: Remove stale workflow labels
        let stale: Vec<String> = current_labels
            .into_iter()
            .filter(|l| l != &target_label)
            .collect();

        if !stale.is_empty() {
            if let Err(e) = platform.remove_labels(issue_id, &stale).await {
                tracing::warn!(
                    issue_id = %issue_id,
                    stale_labels = ?stale,
                    error = %e,
                    "Failed to remove stale workflow labels (target already applied)"
                );
                // Don't fail — the target label is already on the issue.
                // The next transition or verify_final_state will clean up.
            }
        }

        // Step 3: Verify final state
        self.verify_final_state(platform, issue_id, &target_label)
            .await
    }

    /// Verifies that an issue has exactly the expected workflow label.
    ///
    /// Corrective actions:
    /// - If multiple workflow labels exist: removes all except the expected one.
    /// - If zero workflow labels exist (ghost state): re-adds the expected label.
    /// - If exactly the expected label exists: no action needed.
    ///
    /// This method is idempotent and safe to call multiple times.
    pub async fn verify_final_state(
        &self,
        platform: &dyn Platform,
        issue_id: IssueId,
        expected_label: &str,
    ) -> Result<(), PlatformError> {
        let labels = self.get_current_workflow_labels(platform, issue_id).await?;

        match labels.len() {
            // Exactly one label and it's the expected one — all good
            1 if labels[0] == expected_label => {
                tracing::debug!(
                    issue_id = %issue_id,
                    label = expected_label,
                    "Workflow state verified"
                );
                Ok(())
            }
            // Multiple workflow labels — remove extras
            n if n > 1 => {
                let extra: Vec<String> = labels
                    .into_iter()
                    .filter(|l| l != expected_label)
                    .collect();

                tracing::warn!(
                    issue_id = %issue_id,
                    extra_labels = ?extra,
                    "Multiple workflow labels detected, cleaning up"
                );

                platform.remove_labels(issue_id, &extra).await
            }
            // Zero workflow labels (ghost state) — re-add expected
            0 => {
                tracing::warn!(
                    issue_id = %issue_id,
                    expected = expected_label,
                    "Ghost state detected (no workflow labels), recovering"
                );

                platform
                    .add_labels(issue_id, &[expected_label.to_string()])
                    .await
            }
            // Single label but not the expected one — shouldn't happen after
            // set_workflow_state, but handle gracefully
            _ => {
                if labels[0] != expected_label {
                    tracing::warn!(
                        issue_id = %issue_id,
                        found = %labels[0],
                        expected = expected_label,
                        "Unexpected workflow label found, correcting"
                    );
                    platform
                        .add_labels(issue_id, &[expected_label.to_string()])
                        .await?;
                    platform.remove_labels(issue_id, &labels).await?;
                }
                Ok(())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{
        Capability, Comment, CommentId, CreatePrParams, FetchOptions, Issue, PullRequest,
    };
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// A mock platform that records operations for verification.
    struct MockPlatform {
        /// The issue to return from fetch_issue.
        issue: Mutex<Issue>,
        /// Records of add_labels calls: (issue_id, labels).
        added_labels: Mutex<Vec<(IssueId, Vec<String>)>>,
        /// Records of remove_labels calls: (issue_id, labels).
        removed_labels: Mutex<Vec<(IssueId, Vec<String>)>>,
        /// If set, remove_labels will return this error.
        remove_error: Mutex<Option<PlatformError>>,
    }

    impl MockPlatform {
        fn new(issue: Issue) -> Self {
            Self {
                issue: Mutex::new(issue),
                added_labels: Mutex::new(Vec::new()),
                removed_labels: Mutex::new(Vec::new()),
                remove_error: Mutex::new(None),
            }
        }

        fn set_remove_error(&self, err: PlatformError) {
            *self.remove_error.lock().unwrap() = Some(err);
        }

        fn added_labels(&self) -> Vec<(IssueId, Vec<String>)> {
            self.added_labels.lock().unwrap().clone()
        }

        fn removed_labels(&self) -> Vec<(IssueId, Vec<String>)> {
            self.removed_labels.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Platform for MockPlatform {
        fn capabilities(&self) -> Vec<Capability> {
            Vec::new()
        }

        async fn fetch_candidate_issues(
            &self,
            _opts: FetchOptions,
        ) -> Result<Vec<Issue>, PlatformError> {
            Ok(vec![self.issue.lock().unwrap().clone()])
        }

        async fn fetch_issue(&self, _issue_id: IssueId) -> Result<Issue, PlatformError> {
            Ok(self.issue.lock().unwrap().clone())
        }

        async fn fetch_issue_states_by_ids(
            &self,
            _ids: &[IssueId],
        ) -> Result<Vec<Issue>, PlatformError> {
            Ok(vec![self.issue.lock().unwrap().clone()])
        }

        async fn get_workflow_state(
            &self,
            _issue_id: IssueId,
        ) -> Result<Option<String>, PlatformError> {
            Ok(self.issue.lock().unwrap().workflow_state.clone())
        }

        async fn set_workflow_state(
            &self,
            _issue_id: IssueId,
            _state: &str,
        ) -> Result<(), PlatformError> {
            Ok(())
        }

        async fn add_labels(
            &self,
            issue_id: IssueId,
            labels: &[String],
        ) -> Result<(), PlatformError> {
            self.added_labels
                .lock()
                .unwrap()
                .push((issue_id, labels.to_vec()));
            // Also update the mock issue's labels
            let mut issue = self.issue.lock().unwrap();
            for label in labels {
                if !issue.labels.contains(label) {
                    issue.labels.push(label.clone());
                }
            }
            Ok(())
        }

        async fn remove_labels(
            &self,
            issue_id: IssueId,
            labels: &[String],
        ) -> Result<(), PlatformError> {
            // Check if we should return an error
            if let Some(err) = self.remove_error.lock().unwrap().take() {
                return Err(err);
            }
            self.removed_labels
                .lock()
                .unwrap()
                .push((issue_id, labels.to_vec()));
            // Also update the mock issue's labels
            let mut issue = self.issue.lock().unwrap();
            issue.labels.retain(|l| !labels.contains(l));
            Ok(())
        }

        async fn create_comment(
            &self,
            _issue_id: IssueId,
            _body: &str,
        ) -> Result<CommentId, PlatformError> {
            Ok(CommentId(1))
        }

        async fn update_comment(
            &self,
            _comment_id: CommentId,
            _body: &str,
        ) -> Result<(), PlatformError> {
            Ok(())
        }

        async fn find_workpad_comment(
            &self,
            _issue_id: IssueId,
        ) -> Result<Option<(CommentId, String)>, PlatformError> {
            Ok(None)
        }

        async fn list_comments(
            &self,
            _issue_id: IssueId,
        ) -> Result<Vec<Comment>, PlatformError> {
            Ok(Vec::new())
        }

        async fn create_pull_request(
            &self,
            _params: CreatePrParams,
        ) -> Result<PullRequest, PlatformError> {
            Ok(PullRequest {
                id: 1,
                number: 1,
                url: "https://github.com/test/test/pull/1".to_string(),
                state: "open".to_string(),
            })
        }

        async fn validate_credentials(&self) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    fn make_workflow_config() -> WorkflowConfig {
        let mut states = HashMap::new();
        states.insert("backlog".to_string(), "workflow::backlog".to_string());
        states.insert("todo".to_string(), "workflow::todo".to_string());
        states.insert(
            "in_progress".to_string(),
            "workflow::in-progress".to_string(),
        );
        states.insert(
            "human_review".to_string(),
            "workflow::human-review".to_string(),
        );
        states.insert("rework".to_string(), "workflow::rework".to_string());
        states.insert("done".to_string(), "workflow::done".to_string());

        WorkflowConfig {
            states,
            active_states: vec![
                "todo".to_string(),
                "in_progress".to_string(),
                "rework".to_string(),
            ],
            terminal_states: vec!["done".to_string()],
        }
    }

    fn make_issue(labels: Vec<String>) -> Issue {
        Issue {
            id: IssueId(42),
            number: 42,
            title: "Test issue".to_string(),
            description: None,
            url: "https://github.com/test/test/issues/42".to_string(),
            assignee: None,
            workflow_state: None,
            branch_name: "symphony/issue-42".to_string(),
            priority: None,
            labels,
            blocked_by: Vec::new(),
            created_at: None,
            updated_at: None,
        }
    }

    #[tokio::test]
    async fn test_set_workflow_state_from_todo_to_in_progress() {
        let issue = make_issue(vec!["workflow::todo".to_string(), "bug".to_string()]);
        let platform = Arc::new(MockPlatform::new(issue));
        let sm = WorkflowStateMachine::new(make_workflow_config());

        let result = sm
            .set_workflow_state(platform.as_ref(), IssueId(42), "in_progress")
            .await;

        assert!(result.is_ok());

        let added = platform.added_labels();
        assert!(!added.is_empty());
        // First add should be the target label
        assert!(added[0].1.contains(&"workflow::in-progress".to_string()));

        let removed = platform.removed_labels();
        assert!(!removed.is_empty());
        // Should remove the old "workflow::todo" label
        assert!(removed[0].1.contains(&"workflow::todo".to_string()));
    }

    #[tokio::test]
    async fn test_set_workflow_state_no_op_when_already_in_state() {
        let issue = make_issue(vec!["workflow::in-progress".to_string()]);
        let platform = Arc::new(MockPlatform::new(issue));
        let sm = WorkflowStateMachine::new(make_workflow_config());

        let result = sm
            .set_workflow_state(platform.as_ref(), IssueId(42), "in_progress")
            .await;

        assert!(result.is_ok());

        // Should not add or remove anything (no-op)
        let added = platform.added_labels();
        assert!(added.is_empty());
        let removed = platform.removed_labels();
        assert!(removed.is_empty());
    }

    #[tokio::test]
    async fn test_set_workflow_state_invalid_state() {
        let issue = make_issue(vec!["workflow::todo".to_string()]);
        let platform = Arc::new(MockPlatform::new(issue));
        let sm = WorkflowStateMachine::new(make_workflow_config());

        let result = sm
            .set_workflow_state(platform.as_ref(), IssueId(42), "nonexistent")
            .await;

        assert!(matches!(result, Err(PlatformError::MissingState(_))));
    }

    #[tokio::test]
    async fn test_set_workflow_state_remove_failure_still_succeeds() {
        let issue = make_issue(vec!["workflow::todo".to_string()]);
        let platform = Arc::new(MockPlatform::new(issue));
        platform.set_remove_error(PlatformError::PartialLabelUpdate {
            added: vec![],
            failed: vec!["workflow::todo".to_string()],
        });

        let sm = WorkflowStateMachine::new(make_workflow_config());

        // Should still succeed because the target label was added
        let result = sm
            .set_workflow_state(platform.as_ref(), IssueId(42), "in_progress")
            .await;

        // The add succeeded, remove failed but we log and continue.
        // verify_final_state will see the issue still has both labels and try to clean up,
        // but since remove_error was consumed (take()), the second remove will succeed.
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_set_workflow_state_from_no_label() {
        let issue = make_issue(vec!["bug".to_string()]);
        let platform = Arc::new(MockPlatform::new(issue));
        let sm = WorkflowStateMachine::new(make_workflow_config());

        let result = sm
            .set_workflow_state(platform.as_ref(), IssueId(42), "todo")
            .await;

        assert!(result.is_ok());

        let added = platform.added_labels();
        assert!(!added.is_empty());
        assert!(added[0].1.contains(&"workflow::todo".to_string()));

        // No stale labels to remove
        let removed = platform.removed_labels();
        assert!(removed.is_empty());
    }

    #[tokio::test]
    async fn test_get_current_workflow_labels() {
        let issue = make_issue(vec![
            "workflow::todo".to_string(),
            "bug".to_string(),
            "workflow::in-progress".to_string(),
        ]);
        let platform = Arc::new(MockPlatform::new(issue));
        let sm = WorkflowStateMachine::new(make_workflow_config());

        let labels = sm
            .get_current_workflow_labels(platform.as_ref(), IssueId(42))
            .await
            .unwrap();

        assert_eq!(labels.len(), 2);
        assert!(labels.contains(&"workflow::todo".to_string()));
        assert!(labels.contains(&"workflow::in-progress".to_string()));
    }

    #[tokio::test]
    async fn test_verify_final_state_correct() {
        let issue = make_issue(vec!["workflow::todo".to_string()]);
        let platform = Arc::new(MockPlatform::new(issue));
        let sm = WorkflowStateMachine::new(make_workflow_config());

        let result = sm
            .verify_final_state(platform.as_ref(), IssueId(42), "workflow::todo")
            .await;

        assert!(result.is_ok());
        // No corrective actions needed
        assert!(platform.added_labels().is_empty());
        assert!(platform.removed_labels().is_empty());
    }

    #[tokio::test]
    async fn test_verify_final_state_ghost_recovery() {
        // Issue has no workflow labels (ghost state)
        let issue = make_issue(vec!["bug".to_string()]);
        let platform = Arc::new(MockPlatform::new(issue));
        let sm = WorkflowStateMachine::new(make_workflow_config());

        let result = sm
            .verify_final_state(platform.as_ref(), IssueId(42), "workflow::todo")
            .await;

        assert!(result.is_ok());
        // Should have re-added the expected label
        let added = platform.added_labels();
        assert_eq!(added.len(), 1);
        assert!(added[0].1.contains(&"workflow::todo".to_string()));
    }

    #[tokio::test]
    async fn test_verify_final_state_multiple_labels_cleanup() {
        // Issue has multiple workflow labels
        let issue = make_issue(vec![
            "workflow::todo".to_string(),
            "workflow::in-progress".to_string(),
        ]);
        let platform = Arc::new(MockPlatform::new(issue));
        let sm = WorkflowStateMachine::new(make_workflow_config());

        let result = sm
            .verify_final_state(platform.as_ref(), IssueId(42), "workflow::todo")
            .await;

        assert!(result.is_ok());
        // Should have removed the extra label
        let removed = platform.removed_labels();
        assert_eq!(removed.len(), 1);
        assert!(removed[0].1.contains(&"workflow::in-progress".to_string()));
    }

    #[test]
    fn test_workflow_label_resolution() {
        let sm = WorkflowStateMachine::new(make_workflow_config());

        assert_eq!(
            sm.workflow_label("todo").unwrap(),
            "workflow::todo".to_string()
        );
        assert_eq!(
            sm.workflow_label("in_progress").unwrap(),
            "workflow::in-progress".to_string()
        );
        assert!(sm.workflow_label("nonexistent").is_err());
    }

    #[test]
    fn test_all_workflow_label_values() {
        let sm = WorkflowStateMachine::new(make_workflow_config());
        let values = sm.all_workflow_label_values();

        assert!(values.contains(&"workflow::backlog".to_string()));
        assert!(values.contains(&"workflow::todo".to_string()));
        assert!(values.contains(&"workflow::in-progress".to_string()));
        assert!(values.contains(&"workflow::human-review".to_string()));
        assert!(values.contains(&"workflow::rework".to_string()));
        assert!(values.contains(&"workflow::done".to_string()));
        assert_eq!(values.len(), 6);
    }
}
