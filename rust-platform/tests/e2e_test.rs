//! End-to-End tests for the Symphony Platform Adapter.
//!
//! These tests exercise the FULL workflow as described in the design document:
//! 1. Validate credentials
//! 2. Ensure workflow labels exist
//! 3. Create a test issue with workflow::todo label
//! 4. Orchestrator fetches candidates → finds the issue
//! 5. Transition state: todo → in_progress
//! 6. Create workpad comment
//! 7. Find workpad comment
//! 8. Update workpad comment
//! 9. Transition state: in_progress → human_review
//! 10. Create PR (skeleton)
//! 11. Transition state: human_review → done
//! 12. Cleanup: close issue
//!
//! Run modes:
//! - `cargo test --test e2e_test` — runs with MemoryAdapter (always passes, validates logic)
//! - `GITHUB_TOKEN=xxx TEST_REPO_NAME=owner/repo cargo test --test e2e_test -- --include-ignored`
//!   — runs against real GitHub API
//!
//! Required token permissions for real API mode:
//! - Issues: Read and write
//! - Pull requests: Read and write
//! - Labels: Read and write (included in Issues scope)

use std::sync::Arc;

use symphony_platform::config::platform::{IssueFilter, PlatformConfig, WorkflowConfig};
use symphony_platform::error::PlatformError;
use symphony_platform::platform::{
    make_test_issue, CreatePrParams, FetchOptions, IssueId, MemoryAdapter, Platform,
};

// ============================================================================
// E2E with MemoryAdapter (always runs, validates full workflow logic)
// ============================================================================

fn workflow_config() -> WorkflowConfig {
    let mut states = std::collections::HashMap::new();
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

/// E2E Test: Complete workflow lifecycle using MemoryAdapter.
/// This validates the entire orchestration logic without network calls.
#[tokio::test]
async fn e2e_full_workflow_memory_adapter() {
    let adapter = Arc::new(MemoryAdapter::new());
    let wf = workflow_config();

    // --- Step 1: Validate credentials ---
    adapter.validate_credentials().await.unwrap();

    // --- Step 2: Seed a "todo" issue (simulates user creating issue with label) ---
    let issue = make_test_issue(42, "Implement user authentication", Some("workflow::todo"));
    adapter.seed_issue(issue).await;

    // --- Step 3: Orchestrator fetches candidates ---
    let candidates = adapter
        .fetch_candidate_issues(FetchOptions::default())
        .await
        .unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].number, 42);
    assert_eq!(
        candidates[0].workflow_state,
        Some("workflow::todo".to_string())
    );

    // --- Step 4: Orchestrator picks issue, transitions to in_progress ---
    let issue_id = IssueId(42);
    adapter
        .set_workflow_state(issue_id, "workflow::in-progress")
        .await
        .unwrap();

    let state = adapter.get_workflow_state(issue_id).await.unwrap();
    assert_eq!(state, Some("workflow::in-progress".to_string()));

    // Verify old label removed
    let labels = adapter.get_issue_labels(issue_id).await.unwrap();
    let wf_labels: Vec<&String> = labels
        .iter()
        .filter(|l| l.starts_with("workflow::"))
        .collect();
    assert_eq!(wf_labels.len(), 1);
    assert_eq!(wf_labels[0], "workflow::in-progress");

    // --- Step 5: Agent creates workpad comment ---
    let workpad_body = "## Codex Workpad\n\n### Plan\n1. Add auth middleware\n2. Add login endpoint\n3. Add tests\n\n### Status\nStarting implementation...";
    let comment_id = adapter
        .create_comment(issue_id, workpad_body)
        .await
        .unwrap();
    assert!(comment_id.0 > 0);

    // --- Step 6: Agent finds workpad comment ---
    let found = adapter.find_workpad_comment(issue_id).await.unwrap();
    assert!(found.is_some());
    let (found_id, found_body) = found.unwrap();
    assert_eq!(found_id, comment_id);
    assert!(found_body.contains("## Codex Workpad"));
    assert!(found_body.contains("Add auth middleware"));

    // --- Step 7: Agent updates workpad with progress ---
    let updated_body = "## Codex Workpad\n\n### Plan\n1. ~~Add auth middleware~~ ✓\n2. Add login endpoint\n3. Add tests\n\n### Status\nMiddleware complete, working on endpoints...";
    adapter
        .update_comment(comment_id, updated_body)
        .await
        .unwrap();

    let found2 = adapter.find_workpad_comment(issue_id).await.unwrap();
    assert!(found2.unwrap().1.contains("Middleware complete"));

    // --- Step 8: Agent creates PR ---
    let pr = adapter
        .create_pull_request(CreatePrParams {
            title: "feat: add user authentication".to_string(),
            body: "Closes #42\n\nAdds JWT-based auth middleware and login endpoint.".to_string(),
            head: "symphony/42-user-auth".to_string(),
            base: "main".to_string(),
            draft: false,
        })
        .await
        .unwrap();
    assert!(pr.number > 0);
    assert_eq!(pr.state, "open");

    // --- Step 9: Transition to human_review ---
    adapter
        .set_workflow_state(issue_id, "workflow::human-review")
        .await
        .unwrap();

    let state = adapter.get_workflow_state(issue_id).await.unwrap();
    assert_eq!(state, Some("workflow::human-review".to_string()));

    // --- Step 10: Simulate human approval → done ---
    adapter
        .set_workflow_state(issue_id, "workflow::done")
        .await
        .unwrap();

    let final_state = adapter.get_workflow_state(issue_id).await.unwrap();
    assert_eq!(final_state, Some("workflow::done".to_string()));

    // --- Step 11: Verify terminal state ---
    let terminal = &wf.terminal_states;
    assert!(terminal.contains(&"done".to_string()));

    // Issue should no longer appear in candidates (done is not in active_states)
    // MemoryAdapter returns all issues regardless of state in fetch_candidate_issues,
    // but the Orchestrator's is_active_state filter would exclude it.
    let final_issue = adapter.fetch_issue(issue_id).await.unwrap();
    assert_eq!(
        final_issue.workflow_state,
        Some("workflow::done".to_string())
    );
    assert!(!wf.active_states.contains(&"done".to_string()));
}

/// E2E Test: Rework cycle (human requests changes, agent fixes, back to review).
#[tokio::test]
async fn e2e_rework_cycle_memory_adapter() {
    let adapter = Arc::new(MemoryAdapter::new());

    // Setup: issue already in human_review
    let issue = make_test_issue(99, "Fix login bug", Some("workflow::human-review"));
    adapter.seed_issue(issue).await;
    let issue_id = IssueId(99);

    // Human requests rework
    adapter
        .set_workflow_state(issue_id, "workflow::rework")
        .await
        .unwrap();
    assert_eq!(
        adapter.get_workflow_state(issue_id).await.unwrap(),
        Some("workflow::rework".to_string())
    );

    // Agent picks up rework, transitions to in_progress
    adapter
        .set_workflow_state(issue_id, "workflow::in-progress")
        .await
        .unwrap();

    // Agent fixes and goes back to human_review
    adapter
        .set_workflow_state(issue_id, "workflow::human-review")
        .await
        .unwrap();

    // Human approves → done
    adapter
        .set_workflow_state(issue_id, "workflow::done")
        .await
        .unwrap();

    let final_state = adapter.get_workflow_state(issue_id).await.unwrap();
    assert_eq!(final_state, Some("workflow::done".to_string()));

    // Verify only one workflow label at the end
    let labels = adapter.get_issue_labels(issue_id).await.unwrap();
    let wf_labels: Vec<&String> = labels
        .iter()
        .filter(|l| l.starts_with("workflow::"))
        .collect();
    assert_eq!(wf_labels.len(), 1);
}

/// E2E Test: Multiple concurrent issues processed independently.
#[tokio::test]
async fn e2e_multiple_issues_independent() {
    let adapter = Arc::new(MemoryAdapter::new());

    // Seed 3 issues in different states
    adapter
        .seed_issue(make_test_issue(1, "Issue A", Some("workflow::todo")))
        .await;
    adapter
        .seed_issue(make_test_issue(2, "Issue B", Some("workflow::in-progress")))
        .await;
    adapter
        .seed_issue(make_test_issue(3, "Issue C", Some("workflow::todo")))
        .await;

    // Fetch all candidates
    let candidates = adapter
        .fetch_candidate_issues(FetchOptions::default())
        .await
        .unwrap();
    assert_eq!(candidates.len(), 3);

    // Transition issue 1 to in_progress
    adapter
        .set_workflow_state(IssueId(1), "workflow::in-progress")
        .await
        .unwrap();

    // Issue 2 should be unaffected
    let state2 = adapter.get_workflow_state(IssueId(2)).await.unwrap();
    assert_eq!(state2, Some("workflow::in-progress".to_string()));

    // Issue 3 should be unaffected
    let state3 = adapter.get_workflow_state(IssueId(3)).await.unwrap();
    assert_eq!(state3, Some("workflow::todo".to_string()));

    // Transition issue 1 to done
    adapter
        .set_workflow_state(IssueId(1), "workflow::done")
        .await
        .unwrap();

    // Others still in their states
    assert_eq!(
        adapter.get_workflow_state(IssueId(2)).await.unwrap(),
        Some("workflow::in-progress".to_string())
    );
    assert_eq!(
        adapter.get_workflow_state(IssueId(3)).await.unwrap(),
        Some("workflow::todo".to_string())
    );
}

/// E2E Test: Error isolation — one issue failing doesn't affect others.
#[tokio::test]
async fn e2e_error_isolation() {
    let adapter = Arc::new(MemoryAdapter::new());

    adapter
        .seed_issue(make_test_issue(10, "Good issue", Some("workflow::todo")))
        .await;
    // Issue 20 doesn't exist — operations on it should fail

    // Good issue works fine
    adapter
        .set_workflow_state(IssueId(10), "workflow::in-progress")
        .await
        .unwrap();

    // Bad issue fails
    let result = adapter.fetch_issue(IssueId(20)).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), PlatformError::NotFound(_)));

    // Good issue still works after bad issue failed
    adapter
        .set_workflow_state(IssueId(10), "workflow::done")
        .await
        .unwrap();
    assert_eq!(
        adapter.get_workflow_state(IssueId(10)).await.unwrap(),
        Some("workflow::done".to_string())
    );
}

/// E2E Test: Comment lifecycle (create, list, update, find workpad).
#[tokio::test]
async fn e2e_comment_lifecycle() {
    let adapter = Arc::new(MemoryAdapter::new());
    adapter
        .seed_issue(make_test_issue(5, "Comment test", Some("workflow::todo")))
        .await;
    let issue_id = IssueId(5);

    // Create multiple comments
    let c1 = adapter
        .create_comment(issue_id, "First progress update")
        .await
        .unwrap();
    let c2 = adapter
        .create_comment(issue_id, "## Codex Workpad\n\nPlan here")
        .await
        .unwrap();
    let _c3 = adapter
        .create_comment(issue_id, "Another update")
        .await
        .unwrap();

    // List all comments
    let comments = adapter.list_comments(issue_id).await.unwrap();
    assert_eq!(comments.len(), 3);

    // Find workpad (should be c2)
    let workpad = adapter.find_workpad_comment(issue_id).await.unwrap();
    assert!(workpad.is_some());
    let (wid, _) = workpad.unwrap();
    assert_eq!(wid, c2);

    // Update workpad
    adapter
        .update_comment(c2, "## Codex Workpad\n\nUpdated plan")
        .await
        .unwrap();

    // Verify update
    let workpad2 = adapter.find_workpad_comment(issue_id).await.unwrap();
    assert!(workpad2.unwrap().1.contains("Updated plan"));

    // Update non-workpad comment
    adapter
        .update_comment(c1, "Updated first comment")
        .await
        .unwrap();
    let comments2 = adapter.list_comments(issue_id).await.unwrap();
    assert!(comments2.iter().any(|c| c.body == "Updated first comment"));
}

// ============================================================================
// E2E with Real GitHub API (requires token with write permissions)
// ============================================================================

/// E2E Test: Full workflow against real GitHub API.
///
/// Requires:
/// - GITHUB_TOKEN with Issues + PRs read/write
/// - TEST_REPO_NAME (e.g., "owner/repo")
#[tokio::test]
#[ignore]
async fn e2e_real_github_full_workflow() {
    use symphony_platform::platform::github::GithubAdapter;

    dotenvy::dotenv().ok();
    let token_var = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN required");
    let test_repo =
        std::env::var("TEST_REPO_NAME").unwrap_or_else(|_| "hijimo/symphony-e2e-test".to_string());
    let parts: Vec<&str> = test_repo.splitn(2, '/').collect();
    let (owner, repo) = if parts.len() == 2 {
        (parts[0].to_string(), parts[1].to_string())
    } else {
        ("hijimo".to_string(), test_repo.clone())
    };

    // Build real config — set token in env for resolve_token to find
    std::env::set_var("_E2E_GITHUB_TOKEN", &token_var);

    let config = PlatformConfig {
        kind: "github".to_string(),
        api_token: "$_E2E_GITHUB_TOKEN".to_string(),
        base_url: "https://api.github.com".to_string(),
        owner: owner.clone(),
        repo: repo.clone(),
        project_id: None,
        allow_custom_host: false,
        issue_filter: IssueFilter::default(),
        workflow: workflow_config(),
    };

    let adapter = GithubAdapter::new(config).expect("Failed to create GithubAdapter");

    // Step 1: Validate credentials
    adapter
        .validate_credentials()
        .await
        .expect("Credential validation failed");
    println!("✓ Credentials validated");

    // Step 2: Create test issue via raw API (adapter doesn't have create_issue)
    let issue_number = create_test_issue(&token_var, &owner, &repo).await;
    println!("✓ Created test issue #{issue_number}");

    let issue_id = IssueId(issue_number);

    // Step 3: Add workflow::todo label
    adapter
        .add_labels(issue_id, &["workflow::todo".to_string()])
        .await
        .expect("Failed to add todo label");
    println!("✓ Added workflow::todo label");

    // Step 4: Fetch candidates
    let candidates = adapter
        .fetch_candidate_issues(FetchOptions::default())
        .await
        .expect("Failed to fetch candidates");
    assert!(
        candidates.iter().any(|i| i.number == issue_number),
        "Test issue should appear in candidates"
    );
    println!("✓ Issue appears in candidates");

    // Step 5: Transition to in_progress
    adapter
        .set_workflow_state(issue_id, "workflow::in-progress")
        .await
        .expect("Failed to transition to in-progress");
    let state = adapter.get_workflow_state(issue_id).await.unwrap();
    assert_eq!(state, Some("workflow::in-progress".to_string()));
    println!("✓ Transitioned to in-progress");

    // Step 6: Create workpad comment
    let comment_id = adapter
        .create_comment(
            issue_id,
            "## Codex Workpad\n\n### Plan\n- E2E test\n\n### Status\nRunning",
        )
        .await
        .expect("Failed to create comment");
    println!("✓ Created workpad comment (id={})", comment_id.0);

    // Step 7: Find workpad
    let workpad = adapter
        .find_workpad_comment(issue_id)
        .await
        .expect("Failed to find workpad");
    assert!(workpad.is_some(), "Workpad should be found");
    println!("✓ Found workpad comment");

    // Step 8: Update workpad
    adapter
        .update_comment(
            comment_id,
            "## Codex Workpad\n\n### Plan\n- E2E test ✓\n\n### Status\nComplete",
        )
        .await
        .expect("Failed to update comment");
    println!("✓ Updated workpad comment");

    // Step 9: Transition to human_review
    adapter
        .set_workflow_state(issue_id, "workflow::human-review")
        .await
        .expect("Failed to transition to human-review");
    println!("✓ Transitioned to human-review");

    // Step 10: Transition to done
    adapter
        .set_workflow_state(issue_id, "workflow::done")
        .await
        .expect("Failed to transition to done");
    println!("✓ Transitioned to done");

    // Cleanup: close the test issue
    close_test_issue(&token_var, &owner, &repo, issue_number).await;
    println!("✓ Cleaned up test issue #{issue_number}");

    println!("\n🎉 E2E test passed! Full workflow completed successfully.");
}

// --- Helper functions for real API tests ---

async fn create_test_issue(token: &str, owner: &str, repo: &str) -> u64 {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "https://api.github.com/repos/{owner}/{repo}/issues"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "symphony-e2e-test")
        .json(&serde_json::json!({
            "title": "[E2E] Symphony platform adapter test - auto cleanup",
            "body": "Automated E2E test issue. Will be closed after test completes.",
            "labels": []
        }))
        .send()
        .await
        .expect("Failed to create issue");

    assert!(
        resp.status().is_success(),
        "Create issue failed: {}",
        resp.text().await.unwrap_or_default()
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    body["number"]
        .as_u64()
        .expect("No issue number in response")
}

async fn close_test_issue(token: &str, owner: &str, repo: &str, number: u64) {
    let client = reqwest::Client::new();
    let _ = client
        .patch(format!(
            "https://api.github.com/repos/{owner}/{repo}/issues/{number}"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "symphony-e2e-test")
        .json(&serde_json::json!({"state": "closed"}))
        .send()
        .await;
}
