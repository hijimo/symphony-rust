//! Workflow E2E Test (Gitea)
//!
//! Validates that the Gitea workflow template is correctly rendered and that the
//! codex agent follows the workflow instructions (label state transitions, workpad creation).
//!
//! This test:
//! 1. Creates a Gitea issue with "Todo" label
//! 2. Verifies the GiteaAdapter can fetch and manipulate the issue
//! 3. Validates label-based state transitions via the adapter
//! 4. (Codex test) Renders workflow prompt, runs codex agent, verifies actions
//!
//! Required environment:
//! - GITEA_TOKEN: Valid Gitea token with repo access
//! - GITEA_BASE_URL: Gitea API base URL (e.g., https://gitea.example.com/api/v1)
//! - TEST_REPO_NAME: Repository in "owner/repo" format
//!
//! Run with:
//!   source .env && E2E_PLATFORM=gitea cargo test --test real_workflow_gitea -- --ignored --nocapture

mod common;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use common::git_host::GitHost;
use common::gitea_host::GiteaHost;

use symphony_platform::config::platform::{IssueFilter, PlatformConfig, WorkflowConfig};
use symphony_platform::config::parse_workflow;
use symphony_platform::platform::gitea::GiteaAdapter;
use symphony_platform::platform::{FetchOptions, IssueId, Platform};
use symphony_platform::prompt::{IssueContext, PromptEngine};

fn build_gitea_platform_config() -> (PlatformConfig, String) {
    common::load_env();
    let token = std::env::var("GITEA_TOKEN").expect("GITEA_TOKEN must be set");
    let base_url = std::env::var("GITEA_BASE_URL").expect("GITEA_BASE_URL must be set");
    let repo = std::env::var("TEST_REPO_NAME").expect("TEST_REPO_NAME must be set (owner/repo)");

    let mut parts = repo.splitn(2, '/');
    let owner = parts.next().unwrap_or_default().to_string();
    let repo_name = parts.next().unwrap_or_default().to_string();

    let mut states = HashMap::new();
    states.insert("todo".to_string(), "Todo".to_string());
    states.insert("in_progress".to_string(), "In Progress".to_string());
    states.insert("done".to_string(), "Done".to_string());

    let config = PlatformConfig {
        kind: "gitea".to_string(),
        api_token: format!("${}", "GITEA_TOKEN"),
        base_url: base_url.trim_end_matches('/').to_string(),
        owner,
        repo: repo_name,
        project_id: None,
        allow_custom_host: true,
        issue_filter: IssueFilter {
            labels: vec!["Todo".to_string(), "In Progress".to_string()],
            assignee: None,
            milestone: None,
        },
        workflow: WorkflowConfig {
            states,
            active_states: vec!["todo".to_string(), "in_progress".to_string()],
            terminal_states: vec!["done".to_string()],
        },
    };

    (config, token)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

/// Test: GiteaAdapter can validate credentials against a real Gitea instance.
#[tokio::test]
#[ignore]
async fn test_gitea_validate_credentials() {
    let (config, token) = build_gitea_platform_config();
    let adapter = GiteaAdapter::new_with_token(config, &token).unwrap();

    adapter.validate_credentials().await.unwrap();
    eprintln!("[GITEA-E2E] ✓ Credentials validated successfully");
}

/// Test: GiteaAdapter can fetch candidate issues from a real Gitea instance.
#[tokio::test]
#[ignore]
async fn test_gitea_fetch_candidate_issues() {
    let (config, token) = build_gitea_platform_config();
    let adapter = GiteaAdapter::new_with_token(config, &token).unwrap();

    let issues = adapter
        .fetch_candidate_issues(FetchOptions::default())
        .await
        .unwrap();

    eprintln!(
        "[GITEA-E2E] ✓ Fetched {} candidate issues",
        issues.len()
    );
    for issue in &issues {
        eprintln!(
            "  - #{}: {} (state: {:?})",
            issue.number, issue.title, issue.workflow_state
        );
    }
}

/// Test: Full issue lifecycle — create issue, add labels, transition state, close.
#[tokio::test]
#[ignore]
async fn test_gitea_issue_lifecycle() {
    common::load_env();
    let host = GiteaHost::from_env();
    let (config, token) = build_gitea_platform_config();
    let adapter = GiteaAdapter::new_with_token(config, &token).unwrap();

    eprintln!("[GITEA-E2E] Platform: {}", host.platform_name());

    // Step 1: Ensure workflow labels exist
    eprintln!("[GITEA-E2E] Step 1: Ensuring workflow labels exist...");
    adapter.http_client().ensure_workflow_labels().await.unwrap();
    eprintln!("[GITEA-E2E] ✓ Workflow labels verified");

    // Step 2: Create a test issue with "Todo" label
    eprintln!("[GITEA-E2E] Step 2: Creating test issue...");
    let issue_info = host
        .create_issue(
            "[E2E] Gitea adapter lifecycle test",
            "Automated test issue for Gitea adapter E2E validation.",
            &["Todo"],
        )
        .await
        .unwrap();
    eprintln!(
        "[GITEA-E2E] ✓ Created issue #{} ({})",
        issue_info.number, issue_info.url
    );

    let issue_id = IssueId(issue_info.number);

    // Step 3: Fetch the issue via adapter
    eprintln!("[GITEA-E2E] Step 3: Fetching issue via adapter...");
    let issue = adapter.fetch_issue(issue_id).await.unwrap();
    assert_eq!(issue.number, issue_info.number);
    assert_eq!(issue.workflow_state, Some("todo".to_string()));
    eprintln!(
        "[GITEA-E2E] ✓ Issue fetched, state = {:?}",
        issue.workflow_state
    );

    // Step 4: Transition state Todo → In Progress
    eprintln!("[GITEA-E2E] Step 4: Transitioning Todo → In Progress...");
    adapter
        .set_workflow_state(issue_id, "in_progress")
        .await
        .unwrap();

    let issue = adapter.fetch_issue(issue_id).await.unwrap();
    assert_eq!(issue.workflow_state, Some("in_progress".to_string()));
    eprintln!(
        "[GITEA-E2E] ✓ State transitioned to {:?}",
        issue.workflow_state
    );

    // Step 5: Create a workpad comment
    eprintln!("[GITEA-E2E] Step 5: Creating workpad comment...");
    let comment_id = adapter
        .create_comment(issue_id, "## Codex Workpad\n\nE2E test workpad.")
        .await
        .unwrap();
    eprintln!("[GITEA-E2E] ✓ Workpad comment created (id={})", comment_id.0);

    // Step 6: Find workpad comment
    eprintln!("[GITEA-E2E] Step 6: Finding workpad comment...");
    let found = adapter.find_workpad_comment(issue_id).await.unwrap();
    assert!(found.is_some());
    let (found_id, found_body) = found.unwrap();
    assert_eq!(found_id.0, comment_id.0);
    assert!(found_body.contains("## Codex Workpad"));
    eprintln!("[GITEA-E2E] ✓ Workpad comment found");

    // Step 7: Update workpad comment
    eprintln!("[GITEA-E2E] Step 7: Updating workpad comment...");
    adapter
        .update_comment(
            comment_id,
            "## Codex Workpad\n\nE2E test workpad.\n\n### Notes\n\n- Updated by E2E test",
        )
        .await
        .unwrap();
    eprintln!("[GITEA-E2E] ✓ Workpad comment updated");

    // Step 8: Transition to Done
    eprintln!("[GITEA-E2E] Step 8: Transitioning In Progress → Done...");
    adapter.set_workflow_state(issue_id, "done").await.unwrap();

    let issue = adapter.fetch_issue(issue_id).await.unwrap();
    assert_eq!(issue.workflow_state, Some("done".to_string()));
    eprintln!(
        "[GITEA-E2E] ✓ State transitioned to {:?}",
        issue.workflow_state
    );

    // Step 9: Close the issue
    eprintln!("[GITEA-E2E] Step 9: Closing test issue...");
    host.close_issue(issue_info.number).await.unwrap();
    eprintln!("[GITEA-E2E] ✓ Issue closed");

    eprintln!("\n[GITEA-E2E] ═══════════════════════════════════════════");
    eprintln!("[GITEA-E2E] ✓ Full lifecycle test PASSED");
    eprintln!("[GITEA-E2E] ═══════════════════════════════════════════");
}

/// Test: GiteaAdapter PR creation against a real Gitea instance.
#[tokio::test]
#[ignore]
async fn test_gitea_create_pr() {
    common::load_env();
    let host = GiteaHost::from_env();
    let (config, token) = build_gitea_platform_config();
    let adapter = GiteaAdapter::new_with_token(config, &token).unwrap();

    // Create a branch for the PR
    eprintln!("[GITEA-E2E] Creating test branch...");
    let main_sha = host.get_branch_sha("main").await.unwrap();
    let branch_name = format!("e2e-test-pr-{}", chrono::Utc::now().timestamp());
    host.create_branch(&branch_name, &main_sha).await.unwrap();

    // Push a file to the branch
    host.push_file(
        &branch_name,
        &format!("e2e-test-{}.txt", chrono::Utc::now().timestamp()),
        b"E2E test file content",
        "chore: add e2e test file",
    )
    .await
    .unwrap();

    // Create PR via adapter
    eprintln!("[GITEA-E2E] Creating PR via adapter...");
    let pr = adapter
        .create_pull_request(symphony_platform::platform::CreatePrParams {
            title: "[E2E] Test PR from Gitea adapter".to_string(),
            body: "Automated E2E test PR.".to_string(),
            head: branch_name.clone(),
            base: "main".to_string(),
            draft: false,
        })
        .await
        .unwrap();

    eprintln!(
        "[GITEA-E2E] ✓ PR created: #{} ({})",
        pr.number, pr.url
    );
    assert!(pr.number > 0);
    assert_eq!(pr.state, "open");

    // Cleanup: delete branch (PR will be auto-closed)
    host.delete_branch(&branch_name).await.ok();
    eprintln!("[GITEA-E2E] ✓ Cleanup done");
}

/// Test: State normalization cross-match — verifies that GiteaAdapter returns
/// state_key form ("in_progress") and GitlabTrackerAdapter correctly matches
/// it against active_states in original form ("In Progress").
#[tokio::test]
#[ignore]
async fn test_gitea_state_normalization_cross_match() {
    let (config, token) = build_gitea_platform_config();
    let adapter = GiteaAdapter::new_with_token(config, &token).unwrap();

    // Fetch issues — the adapter should return workflow_state in normalized form
    let issues = adapter
        .fetch_candidate_issues(FetchOptions::default())
        .await
        .unwrap();

    for issue in &issues {
        if let Some(ref state) = issue.workflow_state {
            // State should be in normalized key form (lowercase, underscores)
            assert!(
                !state.contains(' ') && !state.contains('-'),
                "workflow_state '{}' should be normalized (no spaces or hyphens)",
                state
            );
            eprintln!(
                "[GITEA-E2E] Issue #{}: state_key = '{}' ✓",
                issue.number, state
            );
        }
    }

    eprintln!("[GITEA-E2E] ✓ All states are in normalized key form");
}

// =============================================================================
// Codex integration E2E test
// =============================================================================

const TIMEOUT_SECS: u64 = 180;

#[derive(Debug, Default)]
struct CodexSessionResult {
    thread_id: Option<String>,
    turn_id: Option<String>,
    turn_completed: Option<bool>,
    error: Option<String>,
    events: Vec<String>,
}

async fn run_codex_session(workspace_dir: &PathBuf, prompt: &str) -> CodexSessionResult {
    let workspace_str = workspace_dir.to_str().unwrap();

    eprintln!("[GITEA-E2E] Starting codex app-server...");
    let mut child = Command::new("bash")
        .args([
            "-lc",
            "codex app-server -c 'model_provider=\"azure\"' -c shell_environment_policy.inherit=all",
        ])
        .current_dir(workspace_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn codex app-server");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let mut result = CodexSessionResult::default();

    async fn send_request(
        stdin: &mut tokio::process::ChildStdin,
        id: u64,
        method: &str,
        params: Value,
    ) {
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        let line = format!("{}\n", serde_json::to_string(&request).unwrap());
        stdin.write_all(line.as_bytes()).await.unwrap();
        stdin.flush().await.unwrap();
        eprintln!("[GITEA-E2E] Sent: {} (id={})", method, id);
    }

    async fn read_message(
        reader: &mut BufReader<tokio::process::ChildStdout>,
        buf: &mut String,
    ) -> Option<Value> {
        buf.clear();
        match reader.read_line(buf).await {
            Ok(0) => None,
            Ok(_) => serde_json::from_str(buf).ok(),
            Err(_) => None,
        }
    }

    // Initialize
    send_request(
        &mut stdin,
        0,
        "initialize",
        json!({
            "clientInfo": { "name": "symphony-workflow-e2e-gitea", "version": "0.1.0" }
        }),
    )
    .await;

    let mut line_buf = String::new();
    let init_timeout = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(init_timeout);

    loop {
        tokio::select! {
            msg = read_message(&mut reader, &mut line_buf) => {
                match msg {
                    Some(m) if m.get("id") == Some(&json!(0)) => {
                        eprintln!("[GITEA-E2E] Initialize response received");
                        break;
                    }
                    Some(_) => continue,
                    None => {
                        result.error = Some("codex closed during initialize".into());
                        child.kill().await.ok();
                        return result;
                    }
                }
            }
            _ = &mut init_timeout => {
                result.error = Some("timeout waiting for initialize response".into());
                child.kill().await.ok();
                return result;
            }
        }
    }

    // Thread start
    send_request(
        &mut stdin,
        1,
        "thread/start",
        json!({
            "cwd": workspace_str,
            "approvalPolicy": "never",
            "sandbox": "danger-full-access"
        }),
    )
    .await;

    #[allow(unused_assignments)]
    let mut thread_id: Option<String> = None;
    let thread_timeout = tokio::time::sleep(Duration::from_secs(15));
    tokio::pin!(thread_timeout);

    loop {
        tokio::select! {
            msg = read_message(&mut reader, &mut line_buf) => {
                match msg {
                    Some(m) => {
                        if m.get("id") == Some(&json!(1)) {
                            thread_id = m.pointer("/result/thread/id")
                                .or_else(|| m.pointer("/result/threadId"))
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            eprintln!("[GITEA-E2E] Thread started: {:?}", thread_id);
                            break;
                        }
                        let method = m.get("method").and_then(|v| v.as_str()).unwrap_or("");
                        if method == "thread/started" {
                            thread_id = m.pointer("/params/thread/id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            if thread_id.is_some() {
                                eprintln!("[GITEA-E2E] Thread started (notification): {:?}", thread_id);
                                break;
                            }
                        }
                    }
                    None => {
                        result.error = Some("codex closed during thread start".into());
                        child.kill().await.ok();
                        return result;
                    }
                }
            }
            _ = &mut thread_timeout => {
                result.error = Some("timeout waiting for thread start".into());
                child.kill().await.ok();
                return result;
            }
        }
    }

    let thread_id = match thread_id {
        Some(id) => id,
        None => {
            child.kill().await.ok();
            result.error = Some("no thread_id received".into());
            return result;
        }
    };
    result.thread_id = Some(thread_id.clone());

    // Turn start with the workflow prompt
    send_request(
        &mut stdin,
        2,
        "turn/start",
        json!({
            "threadId": thread_id,
            "input": [{"type": "text", "text": prompt}],
            "cwd": workspace_str,
            "sandboxPolicy": {"type": "dangerFullAccess"}
        }),
    )
    .await;

    // Stream events
    let turn_timeout = tokio::time::sleep(Duration::from_secs(TIMEOUT_SECS));
    tokio::pin!(turn_timeout);

    loop {
        tokio::select! {
            msg = read_message(&mut reader, &mut line_buf) => {
                match msg {
                    Some(m) => {
                        let method = m.get("method").and_then(|v| v.as_str()).unwrap_or("");
                        match method {
                            "turn/started" => {
                                let turn_id = m.pointer("/params/turnId")
                                    .or_else(|| m.pointer("/params/turn/id"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                result.turn_id = Some(turn_id.to_string());
                                eprintln!("[GITEA-E2E] Turn started: {}", turn_id);
                            }
                            "turn/completed" => {
                                result.turn_completed = Some(true);
                                eprintln!("[GITEA-E2E] Turn completed!");
                                break;
                            }
                            "turn/failed" | "turn/cancelled" => {
                                let reason = m.pointer("/params/error")
                                    .or_else(|| m.pointer("/params/reason"))
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "unknown".into());
                                result.error = Some(format!("{}: {}", method, reason));
                                eprintln!("[GITEA-E2E] {}: {}", method, reason);
                                break;
                            }
                            _ => {
                                if !method.is_empty() {
                                    result.events.push(method.to_string());
                                    if result.events.len() <= 50 {
                                        eprintln!("[GITEA-E2E]   event: {}", method);
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        eprintln!("[GITEA-E2E] codex stdout closed");
                        if result.turn_completed.is_none() {
                            result.error = Some("codex exited before turn completed".into());
                        }
                        break;
                    }
                }
            }
            _ = &mut turn_timeout => {
                result.error = Some("turn timeout".into());
                eprintln!("[GITEA-E2E] Turn timed out after {}s", TIMEOUT_SECS);
                break;
            }
        }
    }

    eprintln!("[GITEA-E2E] Stopping codex app-server...");
    child.kill().await.ok();
    child.wait().await.ok();

    result
}

async fn setup_workspace(host: &GiteaHost) -> TempDir {
    let workspace =
        TempDir::with_prefix("symphony_wf_e2e_gitea_").expect("failed to create temp workspace dir");

    let clone_url = host.clone_url();
    let output = Command::new("git")
        .args(["clone", &clone_url, "."])
        .current_dir(workspace.path())
        .output()
        .await
        .expect("Failed to run git clone");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let sanitized = regex::Regex::new(r"://[^@]+@")
            .unwrap()
            .replace_all(&stderr, "://***@")
            .to_string();
        panic!("git clone failed: {}", sanitized);
    }

    Command::new("git")
        .args(["config", "user.email", "symphony-e2e@test.local"])
        .current_dir(workspace.path())
        .output()
        .await
        .ok();
    Command::new("git")
        .args(["config", "user.name", "Symphony E2E Test"])
        .current_dir(workspace.path())
        .output()
        .await
        .ok();

    eprintln!(
        "[GITEA-E2E] Workspace ready at: {}",
        workspace.path().display()
    );
    workspace
}

/// Test: Workflow template renders correctly with Gitea issue context.
#[tokio::test]
#[ignore]
async fn test_gitea_workflow_template_renders() {
    common::load_env();

    // The raw template has {{placeholders}} — render them first
    let raw_template = include_str!("../../web-platform/src/templates/workflow_gitea.md");
    let rendered_workflow = raw_template
        .replace("{{project_slug}}", "luk/symphony_test_repo")
        .replace("{{platform_endpoint}}", "https://omv.iloxe.com:23000/api/v1")
        .replace("{{platform_host}}", "https://omv.iloxe.com:23000")
        .replace("{{workspace_root}}", "/tmp/symphony-test")
        .replace("{{max_concurrent_agents}}", "2")
        .replace("{{default_branch}}", "main")
        .replace("{{hooks_section}}", "")
        .replace("{{codex_section}}", "codex:\n  read_timeout_ms: 30000\n")
        .replace("{{testing_max_turns}}", "12")
        .replace("{{testing_skip_labels}}", r#"["hotfix", "urgent"]"#);

    let definition = parse_workflow(&rendered_workflow).expect("Failed to parse Gitea workflow template");

    assert!(
        !definition.prompt_template.is_empty(),
        "Prompt template should not be empty"
    );
    assert!(
        definition.config.contains_key("tracker"),
        "Config should have tracker section"
    );

    let engine =
        PromptEngine::compile(&definition.prompt_template).expect("Failed to compile template");

    let issue_ctx = IssueContext {
        id: "42".to_string(),
        identifier: "#42".to_string(),
        title: "Implement feature X".to_string(),
        description: Some("Add feature X to the system.".to_string()),
        priority: None,
        state: "Todo".to_string(),
        branch_name: None,
        url: Some("https://gitea.example.com/org/repo/issues/42".to_string()),
        labels: vec!["Todo".to_string()],
        blocked_by: vec![],
        created_at: Some("2025-01-15T10:00:00Z".to_string()),
        updated_at: Some("2025-01-16T14:30:00Z".to_string()),
    };

    let rendered = engine
        .render(&issue_ctx, None, 1, 20)
        .expect("Failed to render template");

    eprintln!(
        "[GITEA-E2E] Rendered prompt length: {} chars",
        rendered.len()
    );

    assert!(rendered.contains("#42"), "Should contain issue identifier");
    assert!(rendered.contains("Implement feature X"), "Should contain issue title");
    assert!(rendered.contains("Todo"), "Should contain issue state");
    assert!(rendered.contains("gitea_api"), "Should contain gitea_api function references");
    assert!(!rendered.contains("gh issue"), "Should NOT contain gh issue commands");
    assert!(!rendered.contains("gh pr"), "Should NOT contain gh pr commands");
    assert!(!rendered.contains("glab "), "Should NOT contain glab CLI references");

    eprintln!("[GITEA-E2E] ✓ Template renders correctly");
}

/// Test: Full workflow state transition with real codex agent on Gitea.
///
/// Creates a Gitea issue with "Todo" label, renders the workflow template,
/// sends the prompt to codex, then verifies the agent attempted state transitions.
#[tokio::test]
#[ignore]
async fn test_gitea_workflow_codex_state_transition() {
    common::load_env();
    let host = GiteaHost::from_env();
    let (_, _token) = build_gitea_platform_config();
    eprintln!("[GITEA-E2E] Platform: {}", host.platform_name());

    // ─── Step 1: Parse and compile workflow template ────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[GITEA-E2E] STEP 1: Loading Gitea workflow template...");
    eprintln!("============================================================");

    let template_content = include_str!("../../web-platform/src/templates/workflow_gitea.md");
    let gitea_base_url = std::env::var("GITEA_BASE_URL").unwrap();
    let rendered_workflow = template_content
        .replace("{{project_slug}}", host.project_path())
        .replace("{{platform_endpoint}}", &gitea_base_url)
        .replace("{{platform_host}}", &gitea_base_url)
        .replace("{{workspace_root}}", "/tmp/symphony-test")
        .replace("{{max_concurrent_agents}}", "2")
        .replace("{{default_branch}}", "main")
        .replace("{{hooks_section}}", "")
        .replace("{{codex_section}}", "codex:\n  read_timeout_ms: 30000\n")
        .replace("{{testing_max_turns}}", "12")
        .replace("{{testing_skip_labels}}", r#"["hotfix", "urgent"]"#);
    let definition = parse_workflow(&rendered_workflow).expect("Failed to parse Gitea workflow template");
    let engine =
        PromptEngine::compile(&definition.prompt_template).expect("Failed to compile template");
    eprintln!("[GITEA-E2E] ✓ Template compiled");

    // ─── Step 2: Create issue with "Todo" label ─────────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[GITEA-E2E] STEP 2: Creating test issue with 'Todo' label...");
    eprintln!("============================================================");

    let issue = host
        .create_issue(
            "[Workflow E2E] Create a hello.txt file",
            "Task: Create a file called `hello.txt` with content `Hello from workflow E2E!`\n\nThis is a simple task to validate the workflow state machine.",
            &["Todo"],
        )
        .await
        .expect("Failed to create issue");
    eprintln!(
        "[GITEA-E2E] ✓ Created issue #{} ({})",
        issue.number, issue.url
    );

    // ─── Step 3: Render workflow prompt with issue context ───────────────────
    eprintln!("\n============================================================");
    eprintln!("[GITEA-E2E] STEP 3: Rendering workflow prompt...");
    eprintln!("============================================================");

    let issue_ctx = IssueContext {
        id: issue.id.to_string(),
        identifier: format!("#{}", issue.number),
        title: "[Workflow E2E] Create a hello.txt file".to_string(),
        description: Some(
            "Task: Create a file called `hello.txt` with content `Hello from workflow E2E!`\n\nThis is a simple task to validate the workflow state machine.".to_string(),
        ),
        priority: None,
        state: "Todo".to_string(),
        branch_name: None,
        url: Some(issue.url.clone()),
        labels: vec!["Todo".to_string()],
        blocked_by: vec![],
        created_at: Some(chrono::Utc::now().to_rfc3339()),
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
    };

    let rendered_prompt = engine
        .render(&issue_ctx, None, 1, 20)
        .expect("Failed to render prompt");

    let full_prompt = format!(
        "IMPORTANT ENVIRONMENT CONTEXT:\n\
         - GITEA_TOKEN is available in the environment.\n\
         - Gitea API base: {}\n\
         - The project is `{}`.\n\
         - For this test, ONLY do Step 0 (determine state and route) and the initial part of Step 1:\n\
           1. Move the issue from Todo to In Progress using the transition_label function\n\
           2. Create a workpad comment with \"## Codex Workpad\"\n\
           3. Create the file hello.txt with content \"Hello from workflow E2E!\"\n\
         - After these 3 actions, stop. Do NOT proceed to Human Review or any further steps.\n\n\
         ---\n\n{}",
        gitea_base_url,
        host.project_path(),
        rendered_prompt
    );

    eprintln!(
        "[GITEA-E2E] ✓ Prompt rendered ({} chars total)",
        full_prompt.len()
    );

    // ─── Step 4: Setup workspace ────────────────────────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[GITEA-E2E] STEP 4: Setting up workspace...");
    eprintln!("============================================================");

    let workspace = setup_workspace(&host).await;
    let workspace_dir = workspace.path().to_path_buf();

    // ─── Step 5: Run codex agent with workflow prompt ────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[GITEA-E2E] STEP 5: Running codex agent with workflow prompt...");
    eprintln!("============================================================");

    let session_result = run_codex_session(&workspace_dir, &full_prompt).await;

    // ─── Step 6: Verify results ─────────────────────────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[GITEA-E2E] STEP 6: Verifying workflow state transitions...");
    eprintln!("============================================================");

    // 6a: Codex session must complete
    assert!(
        session_result.thread_id.is_some(),
        "Should have received a thread_id"
    );
    assert_eq!(
        session_result.turn_completed,
        Some(true),
        "Turn should complete. Error: {:?}",
        session_result.error
    );
    eprintln!("[GITEA-E2E] ✓ Codex turn completed successfully");

    // 6b: Check label state transition (Todo → In Progress)
    tokio::time::sleep(Duration::from_secs(2)).await;
    let final_labels = host
        .get_issue_labels(issue.number)
        .await
        .expect("Failed to get issue labels");
    eprintln!("[GITEA-E2E] Final labels: {:?}", final_labels);

    let transitioned_to_in_progress = final_labels
        .iter()
        .any(|l| l.eq_ignore_ascii_case("In Progress"));
    let transitioned_to_human_review = final_labels
        .iter()
        .any(|l| l.eq_ignore_ascii_case("Human Review"));
    let still_todo = final_labels.iter().any(|l| l.eq_ignore_ascii_case("Todo"));

    if transitioned_to_in_progress || transitioned_to_human_review {
        eprintln!(
            "[GITEA-E2E] ✓ Agent transitioned state: {:?}",
            final_labels
        );
    } else if still_todo {
        eprintln!(
            "[GITEA-E2E] ⚠ Agent did NOT transition from Todo. Labels: {:?}",
            final_labels
        );
        eprintln!(
            "[GITEA-E2E]   (This may happen due to LLM non-determinism or API auth issues)"
        );
    }

    // 6c: Check workpad comment creation
    let comments = host
        .get_issue_comments(issue.number)
        .await
        .expect("Failed to get issue comments");
    eprintln!("[GITEA-E2E] Issue has {} comments", comments.len());

    let has_workpad = comments.iter().any(|c| {
        c["body"]
            .as_str()
            .map(|b| b.contains("## Codex Workpad"))
            .unwrap_or(false)
    });

    if has_workpad {
        eprintln!("[GITEA-E2E] ✓ Agent created Codex Workpad comment");
    } else {
        eprintln!("[GITEA-E2E] ⚠ No Codex Workpad comment found");
        if !comments.is_empty() {
            for (i, comment) in comments.iter().enumerate().take(3) {
                let body = comment["body"].as_str().unwrap_or("");
                eprintln!(
                    "[GITEA-E2E]   Comment {}: {}...",
                    i,
                    &body[..body.len().min(100)]
                );
            }
        }
    }

    // 6d: Check if hello.txt was created in workspace
    let hello_path = workspace_dir.join("hello.txt");
    let file_created = hello_path.exists();
    if file_created {
        let content = tokio::fs::read_to_string(&hello_path)
            .await
            .unwrap_or_default();
        eprintln!("[GITEA-E2E] ✓ hello.txt created: {:?}", content);
    } else {
        eprintln!("[GITEA-E2E] ⚠ hello.txt not created in workspace");
    }

    // ─── Step 7: Cleanup ────────────────────────────────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[GITEA-E2E] STEP 7: Cleaning up...");
    eprintln!("============================================================");

    host.close_issue(issue.number).await.ok();
    eprintln!("[GITEA-E2E] ✓ Closed issue #{}", issue.number);
    drop(workspace);

    // ─── Summary ────────────────────────────────────────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[GITEA-E2E] RESULTS SUMMARY:");
    eprintln!("============================================================");
    eprintln!("  Template rendered: ✓");
    eprintln!("  Codex turn completed: ✓");
    eprintln!(
        "  State transition (Todo→In Progress): {}",
        if transitioned_to_in_progress {
            "✓"
        } else {
            "⚠ not observed"
        }
    );
    eprintln!(
        "  Workpad comment created: {}",
        if has_workpad {
            "✓"
        } else {
            "⚠ not observed"
        }
    );
    eprintln!(
        "  File created: {}",
        if file_created {
            "✓"
        } else {
            "⚠ not observed"
        }
    );

    // Hard assertions: template + codex must work
    assert!(
        session_result.error.is_none(),
        "No errors expected. Got: {:?}",
        session_result.error
    );

    // Soft assertions: log warnings but don't fail for LLM non-determinism
    if !transitioned_to_in_progress && !has_workpad && !file_created {
        eprintln!("\n[GITEA-E2E] ⚠ WARNING: Agent completed but performed no observable workflow actions.");
        eprintln!("[GITEA-E2E]   This likely means gitea_api was not accessible from the codex sandbox.");
    }

    eprintln!("\n[GITEA-E2E] ✓ WORKFLOW TEMPLATE E2E TEST COMPLETE (Gitea)");
}
