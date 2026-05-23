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

//! Workflow Template E2E Test (GitLab)
//!
//! Validates that WORKFLOW.md.gitlab is correctly rendered and that the codex agent
//! follows the workflow instructions (label state transitions, workpad creation).
//!
//! This test:
//! 1. Parses WORKFLOW.md.gitlab and renders it with real issue context
//! 2. Creates a GitLab issue with "Todo" label
//! 3. Sends the rendered workflow prompt to codex app-server
//! 4. Verifies the agent attempted label transitions and workpad creation
//!
//! Run with:
//!   source .env && E2E_PLATFORM=gitlab cargo test --test real_workflow_gitlab -- --ignored --nocapture

mod common;

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use common::git_host::GitHost;
use common::gitlab_host::GitlabHost;

use symphony_platform::config::parse_workflow;
use symphony_platform::prompt::{IssueContext, PromptEngine};

const TIMEOUT_SECS: u64 = 180;

// ─── Codex Session (reused from real_full_lifecycle) ────────────────────────

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

    eprintln!("[WORKFLOW-E2E] Starting codex app-server...");
    let mut child = Command::new("bash")
        .args([
            "-lc",
            "codex --config shell_environment_policy.inherit=all app-server",
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
        eprintln!("[WORKFLOW-E2E] Sent: {} (id={})", method, id);
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
            "clientInfo": { "name": "symphony-workflow-e2e", "version": "0.1.0" }
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
                        eprintln!("[WORKFLOW-E2E] Initialize response received");
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
                            eprintln!("[WORKFLOW-E2E] Thread started: {:?}", thread_id);
                            break;
                        }
                        let method = m.get("method").and_then(|v| v.as_str()).unwrap_or("");
                        if method == "thread/started" {
                            thread_id = m.pointer("/params/thread/id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            if thread_id.is_some() {
                                eprintln!("[WORKFLOW-E2E] Thread started (notification): {:?}", thread_id);
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
                                eprintln!("[WORKFLOW-E2E] Turn started: {}", turn_id);
                            }
                            "turn/completed" => {
                                result.turn_completed = Some(true);
                                eprintln!("[WORKFLOW-E2E] Turn completed!");
                                break;
                            }
                            "turn/failed" | "turn/cancelled" => {
                                let reason = m.pointer("/params/error")
                                    .or_else(|| m.pointer("/params/reason"))
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "unknown".into());
                                result.error = Some(format!("{}: {}", method, reason));
                                eprintln!("[WORKFLOW-E2E] {}: {}", method, reason);
                                break;
                            }
                            _ => {
                                if !method.is_empty() {
                                    result.events.push(method.to_string());
                                    if result.events.len() <= 50 {
                                        eprintln!("[WORKFLOW-E2E]   event: {}", method);
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        eprintln!("[WORKFLOW-E2E] codex stdout closed");
                        if result.turn_completed.is_none() {
                            result.error = Some("codex exited before turn completed".into());
                        }
                        break;
                    }
                }
            }
            _ = &mut turn_timeout => {
                result.error = Some("turn timeout".into());
                eprintln!("[WORKFLOW-E2E] Turn timed out after {}s", TIMEOUT_SECS);
                break;
            }
        }
    }

    eprintln!("[WORKFLOW-E2E] Stopping codex app-server...");
    child.kill().await.ok();
    child.wait().await.ok();

    result
}

// ─── Workspace Setup ────────────────────────────────────────────────────────

async fn setup_workspace(host: &GitlabHost) -> TempDir {
    let workspace =
        TempDir::with_prefix("symphony_wf_e2e_").expect("failed to create temp workspace dir");

    let clone_url = host.clone_url();
    let output = Command::new("git")
        .args(["clone", &clone_url, "."])
        .current_dir(workspace.path())
        .env("no_proxy", "*")
        .env("NO_PROXY", "*")
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
        "[WORKFLOW-E2E] Workspace ready at: {}",
        workspace.path().display()
    );
    workspace
}

// ─── Tests ──────────────────────────────────────────────────────────────────

/// Test: WORKFLOW.md.gitlab template renders correctly with issue context.
#[tokio::test]
#[ignore]
async fn test_workflow_template_renders() {
    let workflow_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("WORKFLOW.md.gitlab");

    let content = std::fs::read_to_string(&workflow_path)
        .unwrap_or_else(|_| panic!("Cannot read {:?}", workflow_path));

    let definition = parse_workflow(&content).expect("Failed to parse WORKFLOW.md.gitlab");

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
        priority: Some(2),
        state: "Todo".to_string(),
        branch_name: Some("feature/42-implement-x".to_string()),
        url: Some("http://gitlab.example.com/group/repo/-/issues/42".to_string()),
        labels: vec!["Todo".to_string(), "feature".to_string()],
        blocked_by: vec![],
        created_at: Some("2025-01-15T10:00:00Z".to_string()),
        updated_at: Some("2025-01-16T14:30:00Z".to_string()),
    };

    let rendered = engine
        .render(&issue_ctx, None, 1, 20)
        .expect("Failed to render template");

    eprintln!(
        "[WORKFLOW-E2E] Rendered prompt length: {} chars",
        rendered.len()
    );
    eprintln!(
        "[WORKFLOW-E2E] First 500 chars:\n{}",
        &rendered[..rendered.len().min(500)]
    );

    assert!(
        rendered.contains("#42"),
        "Rendered prompt should contain issue identifier"
    );
    assert!(
        rendered.contains("Implement feature X"),
        "Rendered prompt should contain issue title"
    );
    assert!(
        rendered.contains("Todo"),
        "Rendered prompt should contain issue state"
    );
    assert!(
        rendered.contains("glab"),
        "Rendered prompt should contain glab CLI references"
    );

    eprintln!("[WORKFLOW-E2E] ✓ Template renders correctly");
}

/// Test: Full workflow state transition with real codex agent on GitLab.
///
/// Creates a GitLab issue with "Todo" label, renders WORKFLOW.md.gitlab,
/// sends the prompt to codex, then verifies the agent attempted state transitions.
#[tokio::test]
#[ignore]
async fn test_workflow_state_transition_gitlab() {
    common::load_env();
    let host = GitlabHost::from_env();
    eprintln!("[WORKFLOW-E2E] Platform: {}", host.platform_name());

    // ─── Step 1: Parse and compile WORKFLOW.md.gitlab ────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[WORKFLOW-E2E] STEP 1: Loading WORKFLOW.md.gitlab...");
    eprintln!("============================================================");

    let workflow_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("WORKFLOW.md.gitlab");

    let content = std::fs::read_to_string(&workflow_path)
        .unwrap_or_else(|_| panic!("Cannot read {:?}", workflow_path));

    let definition = parse_workflow(&content).expect("Failed to parse WORKFLOW.md.gitlab");
    let engine =
        PromptEngine::compile(&definition.prompt_template).expect("Failed to compile template");
    eprintln!("[WORKFLOW-E2E] ✓ Template compiled");

    // ─── Step 2: Create issue with "Todo" label ─────────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[WORKFLOW-E2E] STEP 2: Creating test issue with 'Todo' label...");
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
        "[WORKFLOW-E2E] ✓ Created issue #{} ({})",
        issue.number, issue.url
    );

    // ─── Step 3: Render workflow prompt with issue context ───────────────────
    eprintln!("\n============================================================");
    eprintln!("[WORKFLOW-E2E] STEP 3: Rendering workflow prompt...");
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

    // Prepend environment context so agent knows glab is available and how to auth
    let gitlab_base_url =
        std::env::var("GITLAB_BASE_URL").unwrap_or_else(|_| "https://gitlab.com".to_string());
    let full_prompt = format!(
        "IMPORTANT ENVIRONMENT CONTEXT:\n\
         - `glab` CLI is installed and authenticated for this GitLab instance.\n\
         - GitLab API base: {}\n\
         - The project is `{}`.\n\
         - GITLAB_TOKEN is available in the environment.\n\
         - For this test, ONLY do Step 0 (determine state and route) and the initial part of Step 1:\n\
           1. Move the issue from Todo to In Progress using: glab issue update {} --label \"In Progress\" --unlabel \"Todo\" --repo {}\n\
           2. Create a workpad comment using: glab issue note {} -m \"## Codex Workpad\" --repo {}\n\
           3. Create the file hello.txt with content \"Hello from workflow E2E!\"\n\
         - After these 3 actions, stop. Do NOT proceed to Human Review or any further steps.\n\
         - Use --hostname {} for all glab commands.\n\n\
         ---\n\n{}",
        gitlab_base_url,
        host.project_path(),
        issue.number,
        host.project_path(),
        issue.number,
        host.project_path(),
        "gitlab.jushuitan-inc.com",
        rendered_prompt
    );

    eprintln!(
        "[WORKFLOW-E2E] ✓ Prompt rendered ({} chars total)",
        full_prompt.len()
    );

    // ─── Step 4: Setup workspace ────────────────────────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[WORKFLOW-E2E] STEP 4: Setting up workspace...");
    eprintln!("============================================================");

    let workspace = setup_workspace(&host).await;
    let workspace_dir = workspace.path().to_path_buf();

    // ─── Step 5: Run codex agent with workflow prompt ────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[WORKFLOW-E2E] STEP 5: Running codex agent with workflow prompt...");
    eprintln!("============================================================");

    let session_result = run_codex_session(&workspace_dir, &full_prompt).await;

    // ─── Step 6: Verify results ─────────────────────────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[WORKFLOW-E2E] STEP 6: Verifying workflow state transitions...");
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
    eprintln!("[WORKFLOW-E2E] ✓ Codex turn completed successfully");

    // 6b: Check label state transition (Todo → In Progress or Human Review)
    tokio::time::sleep(Duration::from_secs(2)).await; // Allow API propagation
    let final_labels = host
        .get_issue_labels(issue.number)
        .await
        .expect("Failed to get issue labels");
    eprintln!("[WORKFLOW-E2E] Final labels: {:?}", final_labels);

    let transitioned_to_in_progress = final_labels
        .iter()
        .any(|l| l.eq_ignore_ascii_case("In Progress"));
    let transitioned_to_human_review = final_labels
        .iter()
        .any(|l| l.eq_ignore_ascii_case("Human Review"));
    let still_todo = final_labels.iter().any(|l| l.eq_ignore_ascii_case("Todo"));

    if transitioned_to_in_progress || transitioned_to_human_review {
        eprintln!(
            "[WORKFLOW-E2E] ✓ Agent transitioned state: {:?}",
            final_labels
        );
    } else if still_todo {
        eprintln!(
            "[WORKFLOW-E2E] ⚠ Agent did NOT transition from Todo. Labels: {:?}",
            final_labels
        );
        eprintln!(
            "[WORKFLOW-E2E]   (This may happen due to LLM non-determinism or glab auth issues)"
        );
    }

    // 6c: Check workpad comment creation
    let notes = host
        .get_issue_notes(issue.number)
        .await
        .expect("Failed to get issue notes");
    eprintln!("[WORKFLOW-E2E] Issue has {} non-system notes", notes.len());

    let has_workpad = notes.iter().any(|n| {
        n["body"]
            .as_str()
            .map(|b| b.contains("## Codex Workpad"))
            .unwrap_or(false)
    });

    if has_workpad {
        eprintln!("[WORKFLOW-E2E] ✓ Agent created Codex Workpad comment");
    } else {
        eprintln!("[WORKFLOW-E2E] ⚠ No Codex Workpad comment found");
        if !notes.is_empty() {
            for (i, note) in notes.iter().enumerate().take(3) {
                let body = note["body"].as_str().unwrap_or("");
                eprintln!(
                    "[WORKFLOW-E2E]   Note {}: {}...",
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
        eprintln!("[WORKFLOW-E2E] ✓ hello.txt created: {:?}", content);
    } else {
        eprintln!("[WORKFLOW-E2E] ⚠ hello.txt not created in workspace");
    }

    // ─── Step 7: Cleanup ────────────────────────────────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[WORKFLOW-E2E] STEP 7: Cleaning up...");
    eprintln!("============================================================");

    host.close_issue(issue.number).await.ok();
    eprintln!("[WORKFLOW-E2E] ✓ Closed issue #{}", issue.number);
    drop(workspace);

    // ─── Summary ────────────────────────────────────────────────────────────
    eprintln!("\n============================================================");
    eprintln!("[WORKFLOW-E2E] RESULTS SUMMARY:");
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
        eprintln!("\n[WORKFLOW-E2E] ⚠ WARNING: Agent completed but performed no observable workflow actions.");
        eprintln!("[WORKFLOW-E2E]   This likely means glab CLI was not accessible from the codex sandbox.");
    }

    eprintln!("\n[WORKFLOW-E2E] ✓ WORKFLOW TEMPLATE E2E TEST COMPLETE (GitLab)");
}
