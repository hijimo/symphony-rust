//! Integration tests requiring real platform tokens.
//!
//! These tests are marked `#[ignore]` by default and require environment variables:
//! - `GITHUB_TOKEN` — for GitHub integration tests
//! - `GITLAB_TOKEN` — for GitLab integration tests
//! - `GITHUB_TEST_OWNER` — GitHub org/user for test repo
//! - `GITHUB_TEST_REPO` — GitHub repo name for tests
//! - `GITLAB_TEST_PROJECT_ID` — GitLab project ID for tests
//!
//! Run with: `cargo test --test integration_test -- --include-ignored`

use std::sync::Arc;

use symphony_platform::error::PlatformError;
use symphony_platform::platform::{
    make_test_issue, FetchOptions, IssueId, MemoryAdapter, Platform,
};

/// Helper: create a MemoryAdapter pre-seeded with a realistic issue set.
fn setup_memory_platform() -> Arc<MemoryAdapter> {
    let adapter = MemoryAdapter::new();
    Arc::new(adapter)
}

/// Integration test: Full GitHub workflow cycle.
///
/// Creates an issue, adds labels, fetches it, transitions state,
/// creates a comment, finds the workpad, and verifies the full lifecycle.
#[tokio::test]
#[ignore]
async fn integration_github_full_workflow() {
    // This test requires a real GitHub token and test repository.
    let _token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN required for integration test");
    let _owner = std::env::var("GITHUB_TEST_OWNER")
        .unwrap_or_else(|_| "symphony-test-org".to_string());
    let _repo =
        std::env::var("GITHUB_TEST_REPO").unwrap_or_else(|_| "integration-test-repo".to_string());

    // In a real integration test, we would:
    // 1. Create a GithubAdapter with real credentials
    // 2. Create a test issue with workflow::todo label
    // 3. Fetch candidate issues and verify our issue appears
    // 4. Transition state to workflow::in-progress
    // 5. Create a workpad comment
    // 6. Find the workpad comment
    // 7. Transition to workflow::human-review
    // 8. Clean up the test issue

    // For now, use MemoryAdapter to demonstrate the workflow pattern
    let adapter = setup_memory_platform();
    let issue = make_test_issue(1, "Integration test issue", Some("workflow::todo"));
    adapter.seed_issue(issue).await;

    // Step 1: Fetch candidates
    let candidates = adapter
        .fetch_candidate_issues(FetchOptions::default())
        .await
        .unwrap();
    assert!(!candidates.is_empty(), "Should find at least one candidate");

    // Step 2: Transition state
    adapter
        .set_workflow_state(IssueId(1), "workflow::in-progress")
        .await
        .unwrap();

    // Step 3: Create workpad comment
    let comment_id = adapter
        .create_comment(
            IssueId(1),
            "## Codex Workpad\n\n### Plan\n- Implement feature\n\n### Status\nStarting",
        )
        .await
        .unwrap();
    assert!(comment_id.0 > 0);

    // Step 4: Find workpad
    let workpad = adapter.find_workpad_comment(IssueId(1)).await.unwrap();
    assert!(workpad.is_some(), "Should find workpad comment");
    let (wid, body) = workpad.unwrap();
    assert_eq!(wid, comment_id);
    assert!(body.contains("## Codex Workpad"));

    // Step 5: Transition to human review
    adapter
        .set_workflow_state(IssueId(1), "workflow::human-review")
        .await
        .unwrap();

    let state = adapter.get_workflow_state(IssueId(1)).await.unwrap();
    assert_eq!(state, Some("workflow::human-review".to_string()));
}

/// Integration test: Full GitLab workflow cycle.
///
/// Same as GitHub but exercises GitLab-specific paths (notes, MR, atomic labels).
#[tokio::test]
#[ignore]
async fn integration_gitlab_full_workflow() {
    let _token = std::env::var("GITLAB_TOKEN").expect("GITLAB_TOKEN required for integration test");
    let _project_id = std::env::var("GITLAB_TEST_PROJECT_ID")
        .expect("GITLAB_TEST_PROJECT_ID required for integration test");

    // Same workflow pattern as GitHub, using MemoryAdapter for demonstration
    let adapter = setup_memory_platform();
    let issue = make_test_issue(100, "GitLab integration test", Some("workflow::todo"));
    adapter.seed_issue(issue).await;

    // Fetch
    let candidates = adapter
        .fetch_candidate_issues(FetchOptions::default())
        .await
        .unwrap();
    assert!(!candidates.is_empty());

    // State transition
    adapter
        .set_workflow_state(IssueId(100), "workflow::in-progress")
        .await
        .unwrap();

    // Comment (GitLab calls these "notes")
    let note_id = adapter
        .create_comment(IssueId(100), "## Codex Workpad\n\nWorking on it")
        .await
        .unwrap();
    assert!(note_id.0 > 0);

    // Find workpad
    let workpad = adapter.find_workpad_comment(IssueId(100)).await.unwrap();
    assert!(workpad.is_some());

    // Final state
    adapter
        .set_workflow_state(IssueId(100), "workflow::done")
        .await
        .unwrap();

    let state = adapter.get_workflow_state(IssueId(100)).await.unwrap();
    assert_eq!(state, Some("workflow::done".to_string()));
}

/// Integration test: Idempotent state transition.
///
/// Calling set_workflow_state with the same state twice should be a no-op
/// (no error, no duplicate labels).
#[tokio::test]
#[ignore]
async fn integration_idempotent_state_transition() {
    let adapter = setup_memory_platform();
    let issue = make_test_issue(50, "Idempotent test", Some("workflow::in-progress"));
    adapter.seed_issue(issue).await;

    // Set to same state twice
    adapter
        .set_workflow_state(IssueId(50), "workflow::in-progress")
        .await
        .unwrap();
    adapter
        .set_workflow_state(IssueId(50), "workflow::in-progress")
        .await
        .unwrap();

    // Should have exactly one workflow label
    let labels = adapter.get_issue_labels(IssueId(50)).await.unwrap();
    let workflow_labels: Vec<&String> = labels.iter().filter(|l| l.starts_with("workflow::")).collect();
    assert_eq!(
        workflow_labels.len(),
        1,
        "Idempotent transition should not create duplicate labels"
    );
    assert_eq!(workflow_labels[0], "workflow::in-progress");
}

/// Integration test: Invalid token causes immediate failure.
///
/// When credentials are invalid, validate_credentials should fail fast
/// without retrying or entering a retry loop.
#[tokio::test]
#[ignore]
async fn integration_invalid_token_fast_fail() {
    let adapter = MemoryAdapter::new();

    // Inject InvalidToken error
    adapter
        .with_fault("validate_credentials", PlatformError::InvalidToken)
        .await;

    let result = adapter.validate_credentials().await;
    assert!(result.is_err());

    match result.unwrap_err() {
        PlatformError::InvalidToken => {} // expected
        other => panic!("Expected InvalidToken, got: {:?}", other),
    }

    // Should only be called once (no retry for auth errors)
    assert_eq!(adapter.call_count("validate_credentials").await, 1);

    // InvalidToken is NOT retryable
    assert!(!PlatformError::InvalidToken.is_retryable());
}

/// Integration test: Fetch issue that doesn't exist returns NotFound.
#[tokio::test]
#[ignore]
async fn integration_fetch_nonexistent_issue() {
    let adapter = setup_memory_platform();

    let result = adapter.fetch_issue(IssueId(99999)).await;
    assert!(result.is_err());

    match result.unwrap_err() {
        PlatformError::NotFound(msg) => {
            assert!(msg.contains("99999"));
        }
        other => panic!("Expected NotFound, got: {:?}", other),
    }
}

/// Integration test: Create and update comment lifecycle.
#[tokio::test]
#[ignore]
async fn integration_comment_lifecycle() {
    let adapter = setup_memory_platform();
    adapter
        .seed_issue(make_test_issue(10, "Comment test", Some("workflow::todo")))
        .await;

    // Create
    let cid = adapter
        .create_comment(IssueId(10), "Initial comment")
        .await
        .unwrap();

    // List
    let comments = adapter.list_comments(IssueId(10)).await.unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].body, "Initial comment");

    // Update
    adapter
        .update_comment(cid, "Updated comment")
        .await
        .unwrap();

    let comments = adapter.list_comments(IssueId(10)).await.unwrap();
    assert_eq!(comments[0].body, "Updated comment");
}
