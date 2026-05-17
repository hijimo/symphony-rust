//! E2E tests: Real WORKFLOW.md template + Real Codex app-server
//!
//! Verifies the full prompt pipeline:
//! 1. Load the actual project WORKFLOW.md
//! 2. Compile the Liquid template via PromptEngine
//! 3. Render with a realistic issue context
//! 4. Send to a real `codex app-server` subprocess
//! 5. Verify the turn completes successfully
//!
//! Tests 1-2 run always (no external deps, catch template syntax errors in CI).
//! Tests 3-4 require `codex` CLI installed and authenticated.
//!
//! Run:
//!   cargo test --test real_workflow_codex                          # non-ignored only
//!   cargo test --test real_workflow_codex -- --ignored --nocapture # real codex tests

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use symphony_platform::config::workflow_loader::load_workflow;
use symphony_platform::prompt::{BlockerContext, IssueContext, PromptEngine};

const TURN_TIMEOUT_SECS: u64 = 120;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn workflow_path() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap().join("WORKFLOW.md")
}

fn make_realistic_issue_context() -> IssueContext {
    IssueContext {
        id: "uuid-e2e-test-001".to_string(),
        identifier: "JST-128".to_string(),
        title: "Add retry logic to webhook delivery".to_string(),
        description: Some(
            "Webhook deliveries currently fail silently on timeout.\n\n\
             Requirements:\n\
             - Retry up to 3 times with exponential backoff\n\
             - Log each retry attempt\n\
             - Mark delivery as failed after exhausting retries"
                .to_string(),
        ),
        priority: Some(2),
        state: "Todo".to_string(),
        labels: vec!["feature".to_string(), "backend".to_string()],
        url: Some("https://linear.app/jst-ai-studio/issue/JST-128".to_string()),
        branch_name: Some("symphony/jst-128".to_string()),
        blocked_by: vec![BlockerContext {
            id: Some("uuid-blocker-001".to_string()),
            identifier: Some("JST-100".to_string()),
            state: Some("Done".to_string()),
        }],
        created_at: Some("2025-06-01T09:00:00Z".to_string()),
        updated_at: Some("2025-06-02T14:30:00Z".to_string()),
    }
}

fn compile_workflow_prompt() -> (PromptEngine, String) {
    let path = workflow_path();
    let workflow = load_workflow(&path)
        .unwrap_or_else(|e| panic!("Failed to load WORKFLOW.md at {}: {}", path.display(), e));

    assert!(
        !workflow.prompt_template.is_empty(),
        "WORKFLOW.md prompt body should not be empty"
    );

    let engine = PromptEngine::compile(&workflow.prompt_template)
        .unwrap_or_else(|e| panic!("Failed to compile WORKFLOW.md template: {}", e));

    (engine, workflow.prompt_template)
}

async fn setup_minimal_workspace() -> TempDir {
    let workspace =
        TempDir::with_prefix("symphony_workflow_e2e_").expect("failed to create temp workspace");

    let ws_path = workspace.path();

    let status = Command::new("git")
        .args(["init"])
        .current_dir(ws_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .expect("failed to run git init");
    assert!(status.success(), "git init failed");

    let status = Command::new("git")
        .args(["config", "user.email", "test@symphony.dev"])
        .current_dir(ws_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .expect("failed to configure git email");
    assert!(status.success());

    let status = Command::new("git")
        .args(["config", "user.name", "Symphony Test"])
        .current_dir(ws_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .expect("failed to configure git name");
    assert!(status.success());

    tokio::fs::write(ws_path.join("README.md"), "# Test Workspace\n")
        .await
        .unwrap();

    let status = Command::new("git")
        .args(["add", "."])
        .current_dir(ws_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .unwrap();
    assert!(status.success());

    let status = Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(ws_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .unwrap();
    assert!(status.success());

    workspace
}

struct CodexSessionResult {
    thread_id: Option<String>,
    turn_completed: bool,
    error: Option<String>,
    events: Vec<String>,
}

async fn run_codex_with_prompt(workspace_dir: &Path, prompt: &str) -> CodexSessionResult {
    let mut result = CodexSessionResult {
        thread_id: None,
        turn_completed: false,
        error: None,
        events: Vec::new(),
    };

    let mut child = Command::new("bash")
        .args(["-lc", "codex app-server"])
        .current_dir(workspace_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn codex app-server — is `codex` installed?");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

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
            Ok(_) => serde_json::from_str(buf.trim()).ok(),
            Err(_) => None,
        }
    }

    // Step 1: initialize
    send_request(
        &mut stdin,
        0,
        "initialize",
        json!({
            "clientInfo": { "name": "symphony-workflow-e2e", "version": "0.1.0" }
        }),
    )
    .await;

    let mut buf = String::new();
    let init_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let msg = tokio::time::timeout_at(init_deadline, read_message(&mut reader, &mut buf)).await;
        match msg {
            Ok(Some(v)) if v.get("id") == Some(&json!(0)) => {
                eprintln!("[WORKFLOW-E2E] Initialize response received");
                break;
            }
            Ok(Some(_)) => continue,
            Ok(None) => {
                result.error = Some("codex closed before initialize response".into());
                child.kill().await.ok();
                return result;
            }
            Err(_) => {
                result.error = Some("initialize timeout".into());
                child.kill().await.ok();
                return result;
            }
        }
    }

    // Step 2: thread/start
    send_request(
        &mut stdin,
        1,
        "thread/start",
        json!({
            "cwd": workspace_dir.to_string_lossy(),
            "approvalPolicy": "never",
            "sandbox": "danger-full-access"
        }),
    )
    .await;

    let thread_deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let msg =
            tokio::time::timeout_at(thread_deadline, read_message(&mut reader, &mut buf)).await;
        match msg {
            Ok(Some(v)) => {
                if v.get("id") == Some(&json!(1)) {
                    result.thread_id = v["result"]["thread"]["id"]
                        .as_str()
                        .or_else(|| v["result"]["threadId"].as_str())
                        .map(|s| s.to_string());
                    eprintln!("[WORKFLOW-E2E] Thread started: {:?}", result.thread_id);
                    break;
                }
                if v.get("method").and_then(|m| m.as_str()) == Some("thread/started") {
                    result.thread_id = v["params"]["thread"]["id"]
                        .as_str()
                        .or_else(|| v["params"]["threadId"].as_str())
                        .map(|s| s.to_string());
                    eprintln!(
                        "[WORKFLOW-E2E] Thread started (notification): {:?}",
                        result.thread_id
                    );
                    break;
                }
            }
            Ok(None) => {
                result.error = Some("codex closed before thread/start response".into());
                child.kill().await.ok();
                return result;
            }
            Err(_) => {
                result.error = Some("thread/start timeout".into());
                child.kill().await.ok();
                return result;
            }
        }
    }

    // Step 3: turn/start with the rendered workflow prompt
    let thread_id = result.thread_id.clone().unwrap_or_default();
    send_request(
        &mut stdin,
        2,
        "turn/start",
        json!({
            "threadId": thread_id,
            "input": [{"type": "text", "text": prompt}],
            "cwd": workspace_dir.to_string_lossy(),
            "sandboxPolicy": {"type": "dangerFullAccess"}
        }),
    )
    .await;

    // Step 4: Stream events until turn completes or timeout
    let turn_deadline = tokio::time::Instant::now() + Duration::from_secs(TURN_TIMEOUT_SECS);

    loop {
        tokio::select! {
            msg = read_message(&mut reader, &mut buf) => {
                match msg {
                    Some(v) => {
                        let method = v.get("method")
                            .and_then(|m| m.as_str())
                            .unwrap_or("");

                        match method {
                            "turn/completed" => {
                                result.turn_completed = true;
                                eprintln!("[WORKFLOW-E2E] Turn completed!");
                                break;
                            }
                            "turn/failed" | "turn/cancelled" => {
                                let reason = v["params"]["error"]
                                    .as_str()
                                    .or_else(|| v["params"]["reason"].as_str())
                                    .unwrap_or("unknown");
                                result.error = Some(format!("{}: {}", method, reason));
                                eprintln!("[WORKFLOW-E2E] Turn failed: {}", reason);
                                break;
                            }
                            _ => {
                                if !method.is_empty() {
                                    result.events.push(method.to_string());
                                    if result.events.len() <= 20 {
                                        eprintln!("[WORKFLOW-E2E]   event: {}", method);
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        if !result.turn_completed {
                            result.error = Some("codex exited before turn completed".into());
                        }
                        break;
                    }
                }
            }
            _ = tokio::time::sleep_until(turn_deadline) => {
                result.error = Some(format!("turn timeout after {}s", TURN_TIMEOUT_SECS));
                eprintln!("[WORKFLOW-E2E] Turn timed out");
                break;
            }
        }
    }

    child.kill().await.ok();
    child.wait().await.ok();

    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 1: WORKFLOW.md template compiles (always runs, no external deps)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_workflow_template_compiles_with_real_file() {
    let path = workflow_path();
    assert!(path.exists(), "WORKFLOW.md not found at {}", path.display());

    let workflow =
        load_workflow(&path).unwrap_or_else(|e| panic!("Failed to load WORKFLOW.md: {}", e));

    assert!(
        !workflow.prompt_template.is_empty(),
        "WORKFLOW.md prompt body is empty — front matter may be malformed"
    );

    let engine = PromptEngine::compile(&workflow.prompt_template);
    assert!(
        engine.is_ok(),
        "WORKFLOW.md template failed to compile: {:?}",
        engine.err()
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 2: Template renders all variables correctly (always runs)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_workflow_template_renders_all_variables() {
    let (engine, _raw_template) = compile_workflow_prompt();
    let ctx = make_realistic_issue_context();

    // First turn (attempt = None)
    let rendered = engine.render(&ctx, None, 1, 20).unwrap();

    assert!(
        rendered.contains("JST-128"),
        "Rendered prompt should contain issue identifier"
    );
    assert!(
        rendered.contains("Add retry logic to webhook delivery"),
        "Rendered prompt should contain issue title"
    );
    assert!(
        rendered.contains("Todo"),
        "Rendered prompt should contain issue state"
    );
    assert!(
        rendered.contains("Webhook deliveries"),
        "Rendered prompt should contain issue description"
    );
    assert!(
        rendered.contains("## Codex Workpad"),
        "Rendered prompt should contain workpad template"
    );
    assert!(
        rendered.contains("Status map"),
        "Rendered prompt should contain status map section"
    );
    assert!(
        !rendered.contains("Continuation context"),
        "First turn should NOT contain continuation context"
    );

    // Retry turn (attempt = Some(2))
    let rendered_retry = engine.render(&ctx, Some(2), 1, 20).unwrap();

    assert!(
        rendered_retry.contains("Continuation context"),
        "Retry attempt should contain continuation context block"
    );
    assert!(
        rendered_retry.contains("retry attempt #2"),
        "Retry attempt should show attempt number"
    );
    assert!(
        rendered_retry.contains("JST-128"),
        "Retry prompt should still contain issue identifier"
    );
}

#[test]
fn test_workflow_template_renders_with_blockers_present() {
    let (engine, _) = compile_workflow_prompt();
    let ctx = make_realistic_issue_context();

    // Verify rendering succeeds even with blockers in context
    // (WORKFLOW.md doesn't iterate blocked_by, but the engine should handle it)
    let rendered = engine.render(&ctx, None, 1, 20);
    assert!(
        rendered.is_ok(),
        "Should render without error when blockers are present"
    );
}

#[test]
fn test_workflow_template_renders_labels() {
    let (engine, _) = compile_workflow_prompt();
    let ctx = make_realistic_issue_context();

    let rendered = engine.render(&ctx, None, 1, 20).unwrap();

    assert!(
        rendered.contains("feature") || rendered.contains("backend"),
        "Rendered prompt should contain at least one label"
    );
}

#[test]
fn test_workflow_template_renders_url() {
    let (engine, _) = compile_workflow_prompt();
    let ctx = make_realistic_issue_context();

    let rendered = engine.render(&ctx, None, 1, 20).unwrap();

    assert!(
        rendered.contains("https://linear.app/jst-ai-studio/issue/JST-128"),
        "Rendered prompt should contain issue URL"
    );
}

#[test]
fn test_workflow_template_renders_nil_description() {
    let (engine, _) = compile_workflow_prompt();
    let mut ctx = make_realistic_issue_context();
    ctx.description = None;

    let rendered = engine.render(&ctx, None, 1, 20).unwrap();

    assert!(
        rendered.contains("No description provided"),
        "Nil description should render fallback text"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 3: Real Codex receives and processes the workflow prompt (#[ignore])
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn test_real_codex_receives_workflow_prompt() {
    let (engine, _) = compile_workflow_prompt();
    let ctx = make_realistic_issue_context();

    let rendered = engine.render(&ctx, None, 1, 20).unwrap();
    eprintln!(
        "[WORKFLOW-E2E] Rendered prompt length: {} chars",
        rendered.len()
    );
    eprintln!(
        "[WORKFLOW-E2E] Prompt preview (first 200 chars):\n{}",
        &rendered[..rendered.len().min(200)]
    );

    let workspace = setup_minimal_workspace().await;
    let ws_path = workspace.path().to_path_buf();
    eprintln!("[WORKFLOW-E2E] Workspace: {}", ws_path.display());

    let result = run_codex_with_prompt(&ws_path, &rendered).await;

    assert!(
        result.thread_id.is_some(),
        "Should have received a thread_id from codex"
    );
    assert!(
        result.turn_completed,
        "Turn should complete. Error: {:?}",
        result.error
    );
    assert!(
        result.error.is_none(),
        "No errors expected. Got: {:?}",
        result.error
    );

    eprintln!(
        "[WORKFLOW-E2E] Turn completed with {} events",
        result.events.len()
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 4: Real Codex handles continuation turn (#[ignore])
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
#[ignore]
async fn test_real_codex_continuation_turn() {
    let (engine, _) = compile_workflow_prompt();
    let mut ctx = make_realistic_issue_context();
    ctx.state = "In Progress".to_string();

    // Render a retry/continuation prompt (attempt=2, turn_number=1)
    // This exercises the {% if attempt %} branch in the template
    let prompt_retry = engine.render(&ctx, Some(2), 1, 20).unwrap();

    assert!(
        prompt_retry.contains("Continuation context"),
        "Retry prompt should contain continuation context block"
    );
    assert!(
        prompt_retry.contains("retry attempt #2"),
        "Retry prompt should show attempt number"
    );

    eprintln!("[WORKFLOW-E2E] Retry prompt: {} chars", prompt_retry.len());

    let workspace = setup_minimal_workspace().await;
    let ws_path = workspace.path().to_path_buf();

    eprintln!("[WORKFLOW-E2E] === Retry turn (attempt=2) ===");
    let result = run_codex_with_prompt(&ws_path, &prompt_retry).await;

    assert!(
        result.thread_id.is_some(),
        "Should have received a thread_id"
    );
    assert!(
        result.turn_completed,
        "Retry turn should complete. Error: {:?}",
        result.error
    );

    eprintln!(
        "[WORKFLOW-E2E] Retry turn completed with {} events",
        result.events.len()
    );
}
