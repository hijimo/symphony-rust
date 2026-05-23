//! In-memory Platform implementation for testing.
//!
//! Provides a fully functional `MemoryAdapter` that stores issues, comments, and labels
//! in memory. Supports fault injection for testing error handling and compensation logic.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::Mutex;

use super::{
    Capability, Comment, CommentId, CreatePrParams, FetchOptions, Issue, IssueId, IssueStatus,
    Platform, PullRequest,
};
use crate::error::PlatformError;

/// Configuration for injecting faults into specific method calls.
#[derive(Debug, Clone)]
pub struct FaultConfig {
    /// Method name to fault (e.g., "add_labels", "remove_labels").
    pub method: String,
    /// The error to return when the fault triggers.
    pub error: PlatformError,
    /// If true, the fault fires on every call (not just the next one).
    pub persistent: bool,
}

/// Internal state held behind `Arc<Mutex<_>>`.
#[derive(Debug)]
pub struct MemoryState {
    pub issues: HashMap<IssueId, Issue>,
    pub comments: HashMap<IssueId, Vec<Comment>>,
    pub pull_requests: Vec<PullRequest>,
    /// Pending faults keyed by method name.
    pub faults: HashMap<String, FaultConfig>,
    /// Call counts per method name.
    pub call_counts: HashMap<String, usize>,
}

impl Default for MemoryState {
    fn default() -> Self {
        Self {
            issues: HashMap::new(),
            comments: HashMap::new(),
            pull_requests: Vec::new(),
            faults: HashMap::new(),
            call_counts: HashMap::new(),
        }
    }
}

/// In-memory Platform adapter for testing.
///
/// Thread-safe via `Arc<Mutex<MemoryState>>`. Supports concurrent access from
/// multiple tokio tasks and fault injection for testing error paths.
#[derive(Debug, Clone)]
pub struct MemoryAdapter {
    state: Arc<Mutex<MemoryState>>,
    next_comment_id: Arc<AtomicU64>,
    next_pr_id: Arc<AtomicU64>,
}

impl MemoryAdapter {
    /// Create a new empty MemoryAdapter.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MemoryState::default())),
            next_comment_id: Arc::new(AtomicU64::new(1000)),
            next_pr_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Set a fault to trigger on the next call to the specified method.
    /// The fault is consumed after one trigger unless `persistent` is true.
    pub async fn with_fault(&self, method: &str, error: PlatformError) {
        let mut state = self.state.lock().await;
        state.faults.insert(
            method.to_string(),
            FaultConfig {
                method: method.to_string(),
                error,
                persistent: false,
            },
        );
    }

    /// Set a persistent fault that triggers on every call to the specified method.
    pub async fn with_persistent_fault(&self, method: &str, error: PlatformError) {
        let mut state = self.state.lock().await;
        state.faults.insert(
            method.to_string(),
            FaultConfig {
                method: method.to_string(),
                error,
                persistent: true,
            },
        );
    }

    /// Clear a previously set fault.
    pub async fn clear_fault(&self, method: &str) {
        let mut state = self.state.lock().await;
        state.faults.remove(method);
    }

    /// Get the number of times a method has been called.
    pub async fn call_count(&self, method: &str) -> usize {
        let state = self.state.lock().await;
        state.call_counts.get(method).copied().unwrap_or(0)
    }

    /// Seed an issue into the in-memory store.
    pub async fn seed_issue(&self, issue: Issue) {
        let mut state = self.state.lock().await;
        state.issues.insert(issue.id, issue);
    }

    /// Seed multiple issues.
    pub async fn seed_issues(&self, issues: Vec<Issue>) {
        let mut state = self.state.lock().await;
        for issue in issues {
            state.issues.insert(issue.id, issue);
        }
    }

    /// Get a snapshot of an issue's current labels.
    pub async fn get_issue_labels(&self, issue_id: IssueId) -> Option<Vec<String>> {
        let state = self.state.lock().await;
        state.issues.get(&issue_id).map(|i| i.labels.clone())
    }

    /// Get direct access to the internal state (for assertions in tests).
    pub async fn snapshot(&self) -> MemoryState {
        let state = self.state.lock().await;
        MemoryState {
            issues: state.issues.clone(),
            comments: state.comments.clone(),
            pull_requests: state.pull_requests.clone(),
            faults: state.faults.clone(),
            call_counts: state.call_counts.clone(),
        }
    }

    /// Check if a fault should fire for the given method. If so, return the error.
    /// Increments call count regardless.
    async fn check_fault(&self, method: &str) -> Option<PlatformError> {
        let mut state = self.state.lock().await;
        // Always increment call count
        *state.call_counts.entry(method.to_string()).or_insert(0) += 1;

        // Check for fault
        if let Some(fault) = state.faults.get(method) {
            let error = fault.error.clone();
            let persistent = fault.persistent;
            if !persistent {
                state.faults.remove(method);
            }
            Some(error)
        } else {
            None
        }
    }
}

impl Default for MemoryAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to create a test issue with sensible defaults.
pub fn make_test_issue(id: u64, title: &str, state: Option<&str>) -> Issue {
    Issue {
        id: IssueId(id),
        number: id,
        title: title.to_string(),
        description: None,
        url: format!("https://example.com/issues/{}", id),
        assignee: None,
        workflow_state: state.map(|s| s.to_string()),
        branch_name: format!("issue-{}", id),
        priority: None,
        labels: state.map(|s| vec![s.to_string()]).unwrap_or_default(),
        blocked_by: Vec::new(),
        created_at: Some(Utc::now()),
        updated_at: Some(Utc::now()),
    }
}

// We need Clone for PlatformError to support fault injection.
// Implement it manually since some variants contain non-Clone types.
impl Clone for PlatformError {
    fn clone(&self) -> Self {
        match self {
            Self::HttpError(s) => Self::HttpError(*s),
            Self::RateLimited { retry_after_ms } => Self::RateLimited {
                retry_after_ms: *retry_after_ms,
            },
            Self::Timeout => Self::Timeout,
            Self::ConnectionRefused => Self::ConnectionRefused,
            Self::ServerError(s) => Self::ServerError(*s),
            Self::CircuitOpen => Self::CircuitOpen,
            Self::InvalidToken => Self::InvalidToken,
            Self::AuthExpired => Self::AuthExpired,
            Self::MissingState(s) => Self::MissingState(s.clone()),
            Self::UnknownAction(s) => Self::UnknownAction(s.clone()),
            Self::Deserialization(e) => {
                // serde_json::Error is not Clone; create a synthetic one
                Self::UnknownAction(format!("deserialization: {}", e))
            }
            Self::Network(_) => Self::ConnectionRefused, // reqwest::Error is not Clone
            Self::PartialLabelUpdate { added, failed } => Self::PartialLabelUpdate {
                added: added.clone(),
                failed: failed.clone(),
            },
            Self::NotFound(s) => Self::NotFound(s.clone()),
            Self::Forbidden(s) => Self::Forbidden(s.clone()),
            Self::Unprocessable(s) => Self::Unprocessable(s.clone()),
            Self::Config(_) => Self::UnknownAction("config error (cloned)".to_string()),
        }
    }
}

#[async_trait]
impl Platform for MemoryAdapter {
    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::AtomicLabels]
    }

    async fn fetch_candidate_issues(
        &self,
        _opts: FetchOptions,
    ) -> Result<Vec<Issue>, PlatformError> {
        if let Some(err) = self.check_fault("fetch_candidate_issues").await {
            return Err(err);
        }
        let state = self.state.lock().await;
        let issues: Vec<Issue> = state.issues.values().cloned().collect();
        Ok(issues)
    }

    async fn fetch_issue(&self, issue_id: IssueId) -> Result<Issue, PlatformError> {
        if let Some(err) = self.check_fault("fetch_issue").await {
            return Err(err);
        }
        let state = self.state.lock().await;
        state
            .issues
            .get(&issue_id)
            .cloned()
            .ok_or_else(|| PlatformError::NotFound(format!("issue {}", issue_id)))
    }

    async fn close_issue(&self, issue_id: IssueId) -> Result<Issue, PlatformError> {
        if let Some(err) = self.check_fault("close_issue").await {
            return Err(err);
        }
        let mut state = self.state.lock().await;
        let issue = state
            .issues
            .get_mut(&issue_id)
            .ok_or_else(|| PlatformError::NotFound(format!("issue {}", issue_id)))?;

        if issue.status() != IssueStatus::Closed {
            issue.labels.retain(|l| !l.starts_with("workflow::"));
            if !issue.labels.iter().any(|l| l == "closed") {
                issue.labels.push("closed".to_string());
            }
            issue.workflow_state = Some("closed".to_string());
            issue.updated_at = Some(Utc::now());
        }

        Ok(issue.clone())
    }

    async fn fetch_issue_states_by_ids(
        &self,
        ids: &[IssueId],
    ) -> Result<Vec<Issue>, PlatformError> {
        if let Some(err) = self.check_fault("fetch_issue_states_by_ids").await {
            return Err(err);
        }
        let state = self.state.lock().await;
        let mut results = Vec::new();
        for id in ids {
            if let Some(issue) = state.issues.get(id) {
                results.push(issue.clone());
            }
        }
        Ok(results)
    }

    async fn get_workflow_state(&self, issue_id: IssueId) -> Result<Option<String>, PlatformError> {
        if let Some(err) = self.check_fault("get_workflow_state").await {
            return Err(err);
        }
        let state = self.state.lock().await;
        let issue = state
            .issues
            .get(&issue_id)
            .ok_or_else(|| PlatformError::NotFound(format!("issue {}", issue_id)))?;
        Ok(issue.workflow_state.clone())
    }

    async fn set_workflow_state(
        &self,
        issue_id: IssueId,
        new_state: &str,
    ) -> Result<(), PlatformError> {
        if let Some(err) = self.check_fault("set_workflow_state").await {
            return Err(err);
        }
        let mut state = self.state.lock().await;
        let issue = state
            .issues
            .get_mut(&issue_id)
            .ok_or_else(|| PlatformError::NotFound(format!("issue {}", issue_id)))?;

        // Remove old workflow labels (those starting with "workflow::")
        issue.labels.retain(|l| !l.starts_with("workflow::"));
        // Add new workflow label
        issue.labels.push(new_state.to_string());
        issue.workflow_state = Some(new_state.to_string());
        issue.updated_at = Some(Utc::now());
        Ok(())
    }

    async fn add_labels(&self, issue_id: IssueId, labels: &[String]) -> Result<(), PlatformError> {
        if labels.is_empty() {
            return Ok(());
        }
        if let Some(err) = self.check_fault("add_labels").await {
            return Err(err);
        }
        let mut state = self.state.lock().await;
        let issue = state
            .issues
            .get_mut(&issue_id)
            .ok_or_else(|| PlatformError::NotFound(format!("issue {}", issue_id)))?;
        for label in labels {
            if !issue.labels.contains(label) {
                issue.labels.push(label.clone());
            }
        }
        issue.updated_at = Some(Utc::now());
        Ok(())
    }

    async fn remove_labels(
        &self,
        issue_id: IssueId,
        labels: &[String],
    ) -> Result<(), PlatformError> {
        if labels.is_empty() {
            return Ok(());
        }
        if let Some(err) = self.check_fault("remove_labels").await {
            return Err(err);
        }
        let mut state = self.state.lock().await;
        let issue = state
            .issues
            .get_mut(&issue_id)
            .ok_or_else(|| PlatformError::NotFound(format!("issue {}", issue_id)))?;
        issue.labels.retain(|l| !labels.contains(l));
        issue.updated_at = Some(Utc::now());
        Ok(())
    }

    async fn create_comment(
        &self,
        issue_id: IssueId,
        body: &str,
    ) -> Result<CommentId, PlatformError> {
        if let Some(err) = self.check_fault("create_comment").await {
            return Err(err);
        }
        let comment_id = CommentId(self.next_comment_id.fetch_add(1, Ordering::SeqCst));
        let comment = Comment {
            id: comment_id,
            body: body.to_string(),
            author: "test-bot".to_string(),
            created_at: Utc::now(),
            is_system: false,
        };
        let mut state = self.state.lock().await;
        state.comments.entry(issue_id).or_default().push(comment);
        Ok(comment_id)
    }

    async fn update_comment(&self, comment_id: CommentId, body: &str) -> Result<(), PlatformError> {
        if let Some(err) = self.check_fault("update_comment").await {
            return Err(err);
        }
        let mut state = self.state.lock().await;
        for comments in state.comments.values_mut() {
            if let Some(c) = comments.iter_mut().find(|c| c.id == comment_id) {
                c.body = body.to_string();
                return Ok(());
            }
        }
        Err(PlatformError::NotFound(format!("comment {}", comment_id)))
    }

    async fn find_workpad_comment(
        &self,
        issue_id: IssueId,
    ) -> Result<Option<(CommentId, String)>, PlatformError> {
        if let Some(err) = self.check_fault("find_workpad_comment").await {
            return Err(err);
        }
        let state = self.state.lock().await;
        if let Some(comments) = state.comments.get(&issue_id) {
            for c in comments {
                if c.body.contains("## Codex Workpad") {
                    return Ok(Some((c.id, c.body.clone())));
                }
            }
        }
        Ok(None)
    }

    async fn list_comments(&self, issue_id: IssueId) -> Result<Vec<Comment>, PlatformError> {
        if let Some(err) = self.check_fault("list_comments").await {
            return Err(err);
        }
        let state = self.state.lock().await;
        Ok(state.comments.get(&issue_id).cloned().unwrap_or_default())
    }

    async fn create_pull_request(
        &self,
        params: CreatePrParams,
    ) -> Result<PullRequest, PlatformError> {
        if let Some(err) = self.check_fault("create_pull_request").await {
            return Err(err);
        }
        let id = self.next_pr_id.fetch_add(1, Ordering::SeqCst);
        let pr = PullRequest {
            id,
            number: id,
            url: format!("https://example.com/pulls/{}", id),
            state: if params.draft {
                "draft".to_string()
            } else {
                "open".to_string()
            },
        };
        let mut state = self.state.lock().await;
        state.pull_requests.push(pr.clone());
        Ok(pr)
    }

    async fn validate_credentials(&self) -> Result<(), PlatformError> {
        if let Some(err) = self.check_fault("validate_credentials").await {
            return Err(err);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_adapter_basic_operations() {
        let adapter = MemoryAdapter::new();
        let issue = make_test_issue(1, "Test issue", Some("workflow::todo"));
        adapter.seed_issue(issue).await;

        let fetched = adapter.fetch_issue(IssueId(1)).await.unwrap();
        assert_eq!(fetched.title, "Test issue");
        assert_eq!(fetched.workflow_state, Some("workflow::todo".to_string()));
    }

    #[tokio::test]
    async fn test_fault_injection_fires_once() {
        let adapter = MemoryAdapter::new();
        adapter
            .seed_issue(make_test_issue(1, "Test", Some("workflow::todo")))
            .await;

        adapter
            .with_fault("fetch_issue", PlatformError::Timeout)
            .await;

        // First call should fail
        let result = adapter.fetch_issue(IssueId(1)).await;
        assert!(result.is_err());

        // Second call should succeed (fault consumed)
        let result = adapter.fetch_issue(IssueId(1)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_persistent_fault() {
        let adapter = MemoryAdapter::new();
        adapter
            .seed_issue(make_test_issue(1, "Test", Some("workflow::todo")))
            .await;

        adapter
            .with_persistent_fault("fetch_issue", PlatformError::ServerError(503))
            .await;

        // Multiple calls should all fail
        for _ in 0..3 {
            let result = adapter.fetch_issue(IssueId(1)).await;
            assert!(result.is_err());
        }
        assert_eq!(adapter.call_count("fetch_issue").await, 3);
    }

    #[tokio::test]
    async fn test_call_count_tracking() {
        let adapter = MemoryAdapter::new();
        adapter
            .seed_issue(make_test_issue(1, "Test", Some("workflow::todo")))
            .await;

        adapter.fetch_issue(IssueId(1)).await.unwrap();
        adapter.fetch_issue(IssueId(1)).await.unwrap();
        adapter.fetch_issue(IssueId(1)).await.unwrap();

        assert_eq!(adapter.call_count("fetch_issue").await, 3);
        assert_eq!(adapter.call_count("add_labels").await, 0);
    }

    #[tokio::test]
    async fn test_label_operations() {
        let adapter = MemoryAdapter::new();
        adapter.seed_issue(make_test_issue(1, "Test", None)).await;

        adapter
            .add_labels(IssueId(1), &["bug".to_string(), "urgent".to_string()])
            .await
            .unwrap();

        let labels = adapter.get_issue_labels(IssueId(1)).await.unwrap();
        assert!(labels.contains(&"bug".to_string()));
        assert!(labels.contains(&"urgent".to_string()));

        adapter
            .remove_labels(IssueId(1), &["bug".to_string()])
            .await
            .unwrap();

        let labels = adapter.get_issue_labels(IssueId(1)).await.unwrap();
        assert!(!labels.contains(&"bug".to_string()));
        assert!(labels.contains(&"urgent".to_string()));
    }

    #[tokio::test]
    async fn test_workflow_state_transition() {
        let adapter = MemoryAdapter::new();
        adapter
            .seed_issue(make_test_issue(1, "Test", Some("workflow::todo")))
            .await;

        adapter
            .set_workflow_state(IssueId(1), "workflow::in-progress")
            .await
            .unwrap();

        let state = adapter.get_workflow_state(IssueId(1)).await.unwrap();
        assert_eq!(state, Some("workflow::in-progress".to_string()));

        // Old label should be removed
        let labels = adapter.get_issue_labels(IssueId(1)).await.unwrap();
        assert!(!labels.contains(&"workflow::todo".to_string()));
        assert!(labels.contains(&"workflow::in-progress".to_string()));
    }

    #[tokio::test]
    async fn test_comment_crud() {
        let adapter = MemoryAdapter::new();
        adapter.seed_issue(make_test_issue(1, "Test", None)).await;

        let cid = adapter
            .create_comment(IssueId(1), "Hello world")
            .await
            .unwrap();

        let comments = adapter.list_comments(IssueId(1)).await.unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].body, "Hello world");

        adapter.update_comment(cid, "Updated body").await.unwrap();

        let comments = adapter.list_comments(IssueId(1)).await.unwrap();
        assert_eq!(comments[0].body, "Updated body");
    }

    #[tokio::test]
    async fn test_workpad_comment_detection() {
        let adapter = MemoryAdapter::new();
        adapter.seed_issue(make_test_issue(1, "Test", None)).await;

        // No workpad yet
        let result = adapter.find_workpad_comment(IssueId(1)).await.unwrap();
        assert!(result.is_none());

        // Create workpad
        adapter
            .create_comment(IssueId(1), "## Codex Workpad\n\nSome content")
            .await
            .unwrap();

        let result = adapter.find_workpad_comment(IssueId(1)).await.unwrap();
        assert!(result.is_some());
        let (_, body) = result.unwrap();
        assert!(body.contains("## Codex Workpad"));
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let adapter = MemoryAdapter::new();
        adapter.seed_issue(make_test_issue(1, "Test", None)).await;

        let mut handles = Vec::new();
        for i in 0..10 {
            let adapter = adapter.clone();
            handles.push(tokio::spawn(async move {
                adapter
                    .add_labels(IssueId(1), &[format!("label-{}", i)])
                    .await
                    .unwrap();
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        let labels = adapter.get_issue_labels(IssueId(1)).await.unwrap();
        assert_eq!(labels.len(), 10);
    }
}
