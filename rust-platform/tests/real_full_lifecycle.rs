//! Full Lifecycle E2E Test
//!
//! This test exercises the COMPLETE Symphony pipeline against real services:
//! 1. Create an issue on the git hosting platform (GitHub or GitLab)
//! 2. Create a workspace and clone the repo
//! 3. Start a real `codex app-server` subprocess
//! 4. Send a thread start + turn with a simple task prompt
//! 5. Stream codex events until turn completes
//! 6. Verify the agent produced output (file change)
//! 7. Push changes and create PR/MR via platform API
//! 8. Clean up (close issue, remove workspace)
//!
//! Requirements:
//! - Platform token env var set (see tests/common/git_host.rs)
//! - `codex` CLI installed and authenticated
//! - Network access to platform API and OpenAI/Anthropic
//!
//! Run with:
//!   cargo test --test real_full_lifecycle -- --ignored --nocapture
//!
//! For GitLab:
//!   E2E_PLATFORM=gitlab E2E_GITLAB_PROJECT=user/repo cargo test --test real_full_lifecycle -- --ignored --nocapture

mod common;

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use common::git_host::{create_git_host, GitHost};

const TIMEOUT_SECS: u64 = 300;

fn sanitize_url_credentials(text: &str) -> String {
    let re = regex::Regex::new(r"://[^@]+@").unwrap();
    re.replace_all(text, "://***@").to_string()
}

// ─── Workspace Helpers ──────────────────────────────────────────────────────

async fn setup_workspace(host: &dyn GitHost) -> TempDir {
    let workspace = TempDir::with_prefix("symphony_e2e_")
        .expect("failed to create temp workspace dir");

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
        let sanitized = sanitize_url_credentials(&stderr);
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
    Command::new("git")
        .args(["config", "credential.helper", ""])
        .current_dir(workspace.path())
        .output()
        .await
        .ok();

    eprintln!("[E2E] Workspace ready at: {}", workspace.path().display());
    workspace
}

// ─── Codex Session ──────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct CodexSessionResult {
    thread_id: Option<String>,
    turn_id: Option<String>,
    turn_completed: Option<bool>,
    error: Option<String>,
    events: Vec<String>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

async fn run_codex_session(workspace_dir: &PathBuf, prompt: &str) -> CodexSessionResult {
    let workspace_str = workspace_dir.to_str().unwrap();

    eprintln!("[E2E] Starting codex app-server...");
    let mut child = Command::new("bash")
        .args(["-lc", "codex app-server"])
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
        eprintln!("[E2E] Sent: {} (id={})", method, id);
    }

    async fn read_message(reader: &mut BufReader<tokio::process::ChildStdout>, buf: &mut String) -> Option<Value> {
        buf.clear();
        match reader.read_line(buf).await {
            Ok(0) => None,
            Ok(_) => serde_json::from_str(buf).ok(),
            Err(_) => None,
        }
    }

    // Initialize
    send_request(&mut stdin, 0, "initialize", json!({
        "clientInfo": { "name": "symphony-e2e-test", "version": "0.1.0" }
    })).await;

    let mut line_buf = String::new();
    let init_timeout = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(init_timeout);

    loop {
        tokio::select! {
            msg = read_message(&mut reader, &mut line_buf) => {
                match msg {
                    Some(m) if m.get("id") == Some(&json!(0)) => {
                        eprintln!("[E2E] Initialize response received");
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
    send_request(&mut stdin, 1, "thread/start", json!({
        "cwd": workspace_str,
        "approvalPolicy": "never",
        "sandbox": "danger-full-access"
    })).await;

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
                            eprintln!("[E2E] Thread started: {:?}", thread_id);
                            break;
                        }
                        let method = m.get("method").and_then(|v| v.as_str()).unwrap_or("");
                        if method == "thread/started" {
                            thread_id = m.pointer("/params/thread/id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            if thread_id.is_some() {
                                eprintln!("[E2E] Thread started (notification): {:?}", thread_id);
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

    // Turn start
    send_request(&mut stdin, 2, "turn/start", json!({
        "threadId": thread_id,
        "input": [{"type": "text", "text": prompt}],
        "cwd": workspace_str,
        "sandboxPolicy": {"type": "dangerFullAccess"}
    })).await;

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
                                eprintln!("[E2E] Turn started: {}", turn_id);
                            }
                            "turn/completed" => {
                                result.turn_completed = Some(true);
                                eprintln!("[E2E] Turn completed successfully!");
                                if let Some(usage) = m.pointer("/params/turn/usage") {
                                    result.input_tokens = usage.get("inputTokens")
                                        .or_else(|| usage.get("input_tokens"))
                                        .and_then(|v| v.as_u64());
                                    result.output_tokens = usage.get("outputTokens")
                                        .or_else(|| usage.get("output_tokens"))
                                        .and_then(|v| v.as_u64());
                                }
                                break;
                            }
                            "turn/failed" | "turn/cancelled" => {
                                let reason = m.pointer("/params/error")
                                    .or_else(|| m.pointer("/params/reason"))
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "unknown".into());
                                result.error = Some(format!("{}: {}", method, reason));
                                eprintln!("[E2E] {}: {}", method, reason);
                                break;
                            }
                            "thread/tokenUsage/updated" => {
                                if let Some(usage) = m.get("params") {
                                    result.input_tokens = usage.get("inputTokens")
                                        .or_else(|| usage.get("input_tokens"))
                                        .and_then(|v| v.as_u64());
                                    result.output_tokens = usage.get("outputTokens")
                                        .or_else(|| usage.get("output_tokens"))
                                        .and_then(|v| v.as_u64());
                                }
                            }
                            _ => {
                                if !method.is_empty() {
                                    result.events.push(method.to_string());
                                    if result.events.len() <= 30 {
                                        eprintln!("[E2E]   event: {}", method);
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        eprintln!("[E2E] codex stdout closed");
                        if result.turn_completed.is_none() {
                            result.error = Some("codex exited before turn completed".into());
                        }
                        break;
                    }
                }
            }
            _ = &mut turn_timeout => {
                result.error = Some("turn timeout".into());
                eprintln!("[E2E] Turn timed out after {}s", TIMEOUT_SECS);
                break;
            }
        }
    }

    eprintln!("[E2E] Stopping codex app-server...");
    child.kill().await.ok();
    child.wait().await.ok();

    result
}

// =============================================================================
// TESTS
// =============================================================================

/// Full lifecycle: issue → workspace → codex agent → verify output → cleanup
#[tokio::test]
#[ignore]
async fn test_full_lifecycle_with_real_codex() {
    let host = create_git_host();
    eprintln!("[E2E] Platform: {}", host.platform_name());

    eprintln!("\n============================================================");
    eprintln!("[E2E] STEP 1: Creating test issue...");
    eprintln!("============================================================");
    let issue = host
        .create_issue(
            "[E2E Test] Create a hello.txt file",
            "Automated E2E test.\n\nTask: Create `hello.txt` with content `Hello from Symphony E2E test!`",
            &["e2e-test"],
        )
        .await
        .expect("Failed to create issue");
    eprintln!("[E2E] Created issue #{} ({})", issue.number, issue.url);

    eprintln!("\n============================================================");
    eprintln!("[E2E] STEP 2: Setting up workspace...");
    eprintln!("============================================================");
    let workspace = setup_workspace(host.as_ref()).await;
    let workspace_dir = workspace.path().to_path_buf();

    eprintln!("\n============================================================");
    eprintln!("[E2E] STEP 3: Running codex agent session...");
    eprintln!("============================================================");
    let prompt = format!(
        "You are working on issue #{}.\n\n\
         Task: Create a file called `hello.txt` in the root of this repository \
         with the content: `Hello from Symphony E2E test!`\n\n\
         Just create the file, nothing else. Do not commit or push.",
        issue.number
    );

    let result = run_codex_session(&workspace_dir, &prompt).await;

    eprintln!("\n============================================================");
    eprintln!("[E2E] STEP 4: Verifying results...");
    eprintln!("============================================================");

    let hello_path = workspace_dir.join("hello.txt");
    let has_output = hello_path.exists();
    if has_output {
        let content = tokio::fs::read_to_string(&hello_path).await.unwrap_or_default();
        eprintln!("[E2E] hello.txt content: {:?}", content);
    }

    eprintln!("\n============================================================");
    eprintln!("[E2E] STEP 5: Cleaning up...");
    eprintln!("============================================================");
    host.close_issue(issue.number).await.ok();
    eprintln!("[E2E] Closed issue #{}", issue.number);
    drop(workspace);

    // Assertions
    assert!(result.thread_id.is_some(), "Should have received a thread_id");
    assert_eq!(result.turn_completed, Some(true), "Turn should complete. Error: {:?}", result.error);
    assert!(has_output, "Agent should have created hello.txt");
    assert!(result.error.is_none(), "No errors expected. Got: {:?}", result.error);

    eprintln!("\n[E2E] ✓ FULL LIFECYCLE TEST PASSED! ({})", host.platform_name());
}

/// Full lifecycle with PR: issue → agent creates file → push via API → create PR/MR
#[tokio::test]
#[ignore]
async fn test_create_musical_notes_file() {
    let host = create_git_host();
    let branch_name = format!("feature/musical-notes-{}", std::process::id());
    eprintln!("[E2E-MUSIC] Platform: {}", host.platform_name());

    eprintln!("\n============================================================");
    eprintln!("[E2E-MUSIC] STEP 1: Creating musical notes issue...");
    eprintln!("============================================================");
    let issue = host
        .create_issue(
            "[Feature] Create a joyful musical notes text art",
            "Create `music.txt` with beautiful musical notes text art.\n\n\
             Requirements:\n\
             - Use musical symbols like ♩ ♪ ♫ ♬ 🎵 🎶 🎼\n\
             - Include a short uplifting melody visualization\n\
             - Keep it under 20 lines",
            &["e2e-test", "feature"],
        )
        .await
        .expect("Failed to create issue");
    eprintln!("[E2E-MUSIC] Created issue #{}", issue.number);

    eprintln!("\n============================================================");
    eprintln!("[E2E-MUSIC] STEP 2: Setting up workspace...");
    eprintln!("============================================================");
    let workspace = setup_workspace(host.as_ref()).await;
    let workspace_dir = workspace.path().to_path_buf();

    eprintln!("\n============================================================");
    eprintln!("[E2E-MUSIC] STEP 3: Running codex agent...");
    eprintln!("============================================================");
    let prompt = format!(
        "You are working on issue #{}.\n\n\
         Task: Create a file called `music.txt` in the root of this repository.\n\
         The file should contain a beautiful, joyful musical notes text art that makes people smile.\n\
         Use musical symbols like ♩ ♪ ♫ ♬ 🎵 🎶 🎼 and create a short uplifting melody visualization.\n\
         Keep it under 20 lines. Be creative and make it feel happy!\n\n\
         Just create the file, nothing else. Do not run any git commands.",
        issue.number
    );

    let result = run_codex_session(&workspace_dir, &prompt).await;

    eprintln!("\n============================================================");
    eprintln!("[E2E-MUSIC] STEP 4: Verifying agent output...");
    eprintln!("============================================================");

    let music_path = workspace_dir.join("music.txt");
    let music_exists = music_path.exists();
    let music_content = if music_exists {
        let content = tokio::fs::read_to_string(&music_path).await.unwrap_or_default();
        eprintln!("[E2E-MUSIC] music.txt content:\n{}", content);
        content
    } else {
        eprintln!("[E2E-MUSIC] music.txt was NOT created!");
        String::new()
    };

    assert_eq!(result.turn_completed, Some(true), "Turn should complete. Error: {:?}", result.error);
    assert!(music_exists, "Agent should have created music.txt");
    assert!(!music_content.is_empty(), "music.txt should not be empty");

    eprintln!("\n============================================================");
    eprintln!("[E2E-MUSIC] STEP 5: Pushing to {} and creating PR/MR...", host.platform_name());
    eprintln!("============================================================");

    let main_sha = host.get_branch_sha("main").await.expect("Failed to get main SHA");
    eprintln!("[E2E-MUSIC] main HEAD SHA: {}", main_sha);

    host.create_branch(&branch_name, &main_sha).await.expect("Failed to create branch");
    eprintln!("[E2E-MUSIC] Created branch: {}", branch_name);

    let commit_msg = format!("feat: add joyful musical notes text art (closes #{})", issue.number);
    host.push_file(&branch_name, "music.txt", music_content.as_bytes(), &commit_msg)
        .await
        .expect("Failed to push file");
    eprintln!("[E2E-MUSIC] ✓ Pushed music.txt to branch");

    tokio::time::sleep(Duration::from_secs(1)).await;

    let pr = host
        .create_pr(
            "feat: add joyful musical notes text art",
            &format!("Closes #{}\n\nAdds a `music.txt` file with beautiful musical note text art.", issue.number),
            &branch_name,
            "main",
        )
        .await
        .expect("Failed to create PR/MR");
    eprintln!("[E2E-MUSIC] ✓ PR/MR created: #{} - {}", pr.number, pr.url);

    eprintln!("\n============================================================");
    eprintln!("[E2E-MUSIC] STEP 6: Cleaning up...");
    eprintln!("============================================================");
    host.close_issue(issue.number).await.ok();
    host.delete_branch(&branch_name).await.ok();
    eprintln!("[E2E-MUSIC] Closed issue #{}, deleted branch {}", issue.number, branch_name);
    drop(workspace);

    eprintln!("\n============================================================");
    eprintln!("[E2E-MUSIC] RESULTS:");
    eprintln!("============================================================");
    eprintln!("  Platform: {}", host.platform_name());
    eprintln!("  Thread ID: {:?}", result.thread_id);
    eprintln!("  Turn ID: {:?}", result.turn_id);
    eprintln!("  Turn completed: {:?}", result.turn_completed);
    eprintln!("  Events received: {}", result.events.len());
    eprintln!("  music.txt: {} bytes", music_content.len());
    eprintln!("  PR/MR: #{} - {}", pr.number, pr.url);

    assert!(result.error.is_none(), "No errors expected. Got: {:?}", result.error);

    eprintln!("\n[E2E-MUSIC] ✓ FULL LIFECYCLE WITH PR TEST PASSED! ({})", host.platform_name());
}

/// Minimal smoke test: codex app-server starts and responds
#[tokio::test]
#[ignore]
async fn test_codex_app_server_starts() {
    let workspace = std::env::temp_dir().join("symphony_e2e_smoke");
    tokio::fs::create_dir_all(&workspace).await.unwrap();

    let mut child = Command::new("bash")
        .args(["-lc", "codex app-server"])
        .current_dir(&workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("codex app-server should be available");

    tokio::time::sleep(Duration::from_millis(500)).await;

    let status = child.try_wait().unwrap();
    assert!(
        status.is_none(),
        "codex app-server should still be running, but exited with: {:?}",
        status
    );

    child.kill().await.ok();
    tokio::fs::remove_dir_all(&workspace).await.ok();
    eprintln!("[E2E] ✓ codex app-server starts successfully");
}

/// Verify platform API access with the configured token
#[tokio::test]
#[ignore]
async fn test_platform_api_access() {
    let host = create_git_host();
    eprintln!("[E2E] Testing {} API access...", host.platform_name());

    let sha = host.get_branch_sha("main").await;
    assert!(sha.is_ok(), "Should be able to read main branch. Error: {:?}", sha.err());
    eprintln!("[E2E] ✓ {} API access confirmed. main SHA: {}", host.platform_name(), sha.unwrap());
}
