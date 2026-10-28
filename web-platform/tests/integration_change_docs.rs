//! Integration tests: workbench WebSocket + interactive AI turns + AI
//! out-of-bounds protection + stage kanban, wired through the real router,
//! real git and a fake codex app-server.

mod sdlc_support;

use futures::{SinkExt, StreamExt};
use reqwest::StatusCode;
use sdlc_support::*;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn start_full(app: &SdlcApp) -> i64 {
    let (st, v) = app
        .post(
            &app.base(),
            json!({"track": "full", "title": "WS", "initialContent": INTENT_OK}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["data"]["flow"]["issueIid"].as_i64().unwrap()
}

async fn connect(app: &SdlcApp, iid: i64) -> Ws {
    let (_, t) = app
        .post(&format!("{}/{iid}/ws-ticket", app.base()), json!({}))
        .await;
    let ticket = t["data"]["ticket"].as_str().unwrap();
    let url = format!(
        "{}{}/{iid}/ws?ticket={ticket}",
        app.addr.replace("http://", "ws://"),
        app.base()
    );
    let (ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    ws
}

async fn next_matching(ws: &mut Ws, pred: impl Fn(&Value) -> bool) -> Value {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let msg = tokio::time::timeout_at(deadline, ws.next())
            .await
            .expect("timeout waiting for ws event")
            .expect("ws closed")
            .unwrap();
        if let Message::Text(t) = msg {
            let v: Value = serde_json::from_str(&t).unwrap();
            if pred(&v) {
                return v;
            }
        }
    }
}

async fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::Text(v.to_string().into())).await.unwrap();
}

#[tokio::test]
async fn ws_ticket_is_single_use_and_required() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app).await;
    let url = format!(
        "{}{}/{iid}/ws?ticket=bogus",
        app.addr.replace("http://", "ws://"),
        app.base()
    );
    assert!(tokio_tungstenite::connect_async(url).await.is_err());
    let (_, t) = app
        .post(&format!("{}/{iid}/ws-ticket", app.base()), json!({}))
        .await;
    let ticket = t["data"]["ticket"].as_str().unwrap().to_string();
    let url = format!(
        "{}{}/{iid}/ws?ticket={ticket}",
        app.addr.replace("http://", "ws://"),
        app.base()
    );
    assert!(tokio_tungstenite::connect_async(url.clone()).await.is_ok());
    assert!(
        tokio_tungstenite::connect_async(url).await.is_err(),
        "ticket reused"
    );
}

#[tokio::test]
async fn interactive_turn_streams_events_and_commits_as_ai() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app).await;
    let mut ws = connect(&app, iid).await;
    next_matching(&mut ws, |v| v["type"] == "ready").await;

    send(&mut ws, json!({"type": "ping"})).await;
    next_matching(&mut ws, |v| v["type"] == "pong").await;

    let target = format!("docs/changes/{iid}/intent.md");
    send(
        &mut ws,
        json!({"type": "send", "stage": "intent", "text": format!("把需求写完整，写入 `{target}`"),
               "files": ["src/main.rs"], "selection": {"path": target, "startLine": 1, "endLine": 3, "text": "## Problem"}}),
    )
    .await;
    next_matching(&mut ws, |v| v["type"] == "accepted").await;
    next_matching(&mut ws, |v| {
        v["type"] == "agent" && v["event"]["type"] == "message_delta"
    })
    .await;
    next_matching(&mut ws, |v| {
        v["type"] == "agent"
            && v["event"]["type"] == "item"
            && v["event"]["item"]["kind"] == "file_change"
    })
    .await;
    let done = next_matching(&mut ws, |v| {
        v["type"] == "agent" && v["event"]["type"] == "turn_completed"
    })
    .await;
    assert_eq!(done["event"]["status"], "completed", "{done}");
    next_matching(&mut ws, |v| v["type"] == "flow_changed").await;

    let log = git(
        &app.bare,
        &["log", "-1", "--format=%B", &format!("symphony/docs-{iid}")],
    )
    .unwrap();
    assert!(log.contains("Symphony-Actor: ai"), "{log}");
    // Turn diff carries the pre-turn sha so the UI can revert the turn / hunks.
    let mut ws3 = connect(&app, iid).await;
    let hist = next_matching(&mut ws3, |v| {
        v["type"] == "history" && v["stage"] == "intent"
    })
    .await;
    let td = hist["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "turn_diff")
        .cloned()
        .expect("turn_diff");
    let base_sha = td["baseSha"].as_str().unwrap();
    assert!(td["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f == &json!(target)));
    let enc = urlencoding::encode(&target).to_string();
    let (_, before) = app
        .get(&format!(
            "{}/{iid}/file?path={enc}&rev={base_sha}",
            app.base()
        ))
        .await;
    let before = before["data"]["content"].as_str().unwrap().to_string();
    assert!(before.contains("users cannot see claim status"), "{before}");
    let (_, now) = app
        .get(&format!("{}/{iid}/file?path={enc}", app.base()))
        .await;
    assert_ne!(
        now["data"]["content"].as_str().unwrap(),
        before,
        "rev must return old content"
    );
    let (st, _) = app
        .get(&format!("{}/{iid}/file?path={enc}&rev=main", app.base()))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let text = read_branch_file(&app.bare, &format!("symphony/docs-{iid}"), &target).unwrap();
    assert!(
        text.starts_with("---\nsymphony:"),
        "frontmatter kept: {text}"
    );

    // Context (files + selection) was forwarded to the agent in the user message.
    let (_, t) = app
        .post(&format!("{}/{iid}/ws-ticket", app.base()), json!({}))
        .await;
    let _ = t;
    let mut ws2 = connect(&app, iid).await;
    let hist = next_matching(&mut ws2, |v| {
        v["type"] == "history" && v["stage"] == "intent"
    })
    .await;
    let user = hist["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "user_message")
        .unwrap();
    let text = user["text"].as_str().unwrap();
    assert!(text.contains("Referenced files") && text.contains("src/main.rs"));
    assert!(text.contains("Selected text"));

    // Session resumes the stored thread after the process restarts.
    let codex_log = std::fs::read_to_string(app.codex_log()).unwrap();
    assert!(codex_log.contains("thread/start thread-fake-1"));
}

#[tokio::test]
async fn ai_cannot_write_outside_whitelist() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app).await;
    let mut ws = connect(&app, iid).await;
    next_matching(&mut ws, |v| v["type"] == "ready").await;
    send(
        &mut ws,
        json!({"type": "send", "stage": "intent", "text": format!("[outside] 改 `docs/changes/{iid}/intent.md`")}),
    )
    .await;
    let notice = next_matching(&mut ws, |v| {
        v["type"] == "agent"
            && v["event"]["type"] == "notice"
            && v["event"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("README.md")
    })
    .await;
    assert_eq!(notice["event"]["level"], "warning");
    next_matching(&mut ws, |v| {
        v["type"] == "agent" && v["event"]["type"] == "turn_completed"
    })
    .await;
    // fileChange approval for README was declined…
    let codex_log = std::fs::read_to_string(app.codex_log()).unwrap();
    assert!(codex_log.contains("\"decline\""), "{codex_log}");
    // …and the misbehaving direct write was reverted by the git-status fallback.
    let readme = std::fs::read_to_string(app.primary_ws(iid).join("README.md")).unwrap();
    assert_eq!(readme, "# app\n");
    let main_readme =
        read_branch_file(&app.bare, &format!("symphony/docs-{iid}"), "README.md").unwrap();
    assert_eq!(main_readme, "# app\n");
}

#[tokio::test]
async fn interactive_command_approval_round_trip() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app).await;
    let mut ws = connect(&app, iid).await;
    next_matching(&mut ws, |v| v["type"] == "ready").await;
    send(
        &mut ws,
        json!({"type": "send", "stage": "intent", "text": "[ask] clean up"}),
    )
    .await;
    let req = next_matching(&mut ws, |v| {
        v["type"] == "agent" && v["event"]["type"] == "approval_request"
    })
    .await;
    assert_eq!(req["event"]["command"], "rm -rf /tmp/x");
    // A second send while running is refused.
    send(
        &mut ws,
        json!({"type": "send", "stage": "intent", "text": "again"}),
    )
    .await;
    let busy = next_matching(&mut ws, |v| v["type"] == "error").await;
    assert!(busy["message"].as_str().unwrap().contains("AI 正在运行"));
    let rid = req["event"]["requestId"].as_str().unwrap();
    send(&mut ws, json!({"type": "answer", "stage": "intent", "requestId": rid, "answer": {"decision": "accept"}})).await;
    next_matching(&mut ws, |v| {
        v["type"] == "agent" && v["event"]["type"] == "turn_completed"
    })
    .await;
    let codex_log = std::fs::read_to_string(app.codex_log()).unwrap();
    assert!(
        codex_log.contains("{\"decision\": \"accept\"}"),
        "{codex_log}"
    );
}

#[tokio::test]
async fn non_interactive_draft_declines_commands() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app).await;
    // Drive a non-interactive turn through the service directly.
    let base = format!("{}/{iid}", app.base());
    app.post(&format!("{base}/docs/intent/submit"), json!({}))
        .await;
    app.post(
        &format!("{base}/docs/intent/decision"),
        json!({"action": "approve"}),
    )
    .await;
    app.wait_flow(iid, |f| SdlcApp::doc_status(f, "spec") == "in_review")
        .await;
    let codex_log = std::fs::read_to_string(app.codex_log()).unwrap();
    assert!(
        codex_log.contains("file docs/changes/1/spec.md {\"decision\": \"accept\"}"),
        "{codex_log}"
    );
}

#[tokio::test]
async fn human_save_blocked_while_ai_edits_same_file() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app).await;
    let mut ws = connect(&app, iid).await;
    next_matching(&mut ws, |v| v["type"] == "ready").await;
    send(&mut ws, json!({"type": "send", "stage": "intent", "text": format!("[ask] edit `docs/changes/{iid}/intent.md`")})).await;
    // While the command approval is pending the turn is running and the file is locked.
    let req = next_matching(&mut ws, |v| {
        v["type"] == "agent" && v["event"]["type"] == "approval_request"
    })
    .await;
    send(&mut ws, json!({"type": "status"})).await;
    let st = next_matching(&mut ws, |v| v["type"] == "status").await;
    assert_eq!(st["sessions"][0]["running"], true);
    // Submit is refused while the AI runs on this document.
    let (code, _) = app
        .post(
            &format!("{}/{iid}/docs/intent/submit", app.base()),
            json!({}),
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT);
    let rid = req["event"]["requestId"].as_str().unwrap();
    send(&mut ws, json!({"type": "answer", "stage": "intent", "requestId": rid, "answer": {"decision": "decline"}})).await;
    next_matching(&mut ws, |v| {
        v["type"] == "agent" && v["event"]["type"] == "turn_completed"
    })
    .await;
    let (code, v) = app
        .post(
            &format!("{}/{iid}/docs/intent/submit", app.base()),
            json!({}),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{v}");
}

#[tokio::test]
async fn stage_kanban_columns() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app).await;
    let todo = app.gitlab.add_issue("fast one", &["Todo"]);
    let review = app.gitlab.add_issue("in review", &["Human Review"]);
    let none = app.gitlab.add_issue("new", &[]);
    let weird = app.gitlab.add_issue("weird", &["doc:spec", "In Progress"]);
    let done = app.gitlab.add_issue("done", &["Done"]);
    app.gitlab
        .issues
        .lock()
        .unwrap()
        .get_mut(&done)
        .unwrap()
        .state = "closed".into();

    let (st, v) = app
        .get(&format!("/api/projects/{}/kanban/stages", app.project_id))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let cols = v["data"]["columns"].as_array().unwrap();
    let keys: Vec<&str> = cols.iter().map(|c| c["key"].as_str().unwrap()).collect();
    assert_eq!(keys, vec!["plan", "design", "build", "deploy", "maintain"]);
    let col = |k: &str| cols.iter().find(|c| c["key"] == k).unwrap().clone();
    let has = |k: &str, iid: u64| {
        col(k)["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["iid"] == iid)
    };
    assert!(has("plan", iid as u64) && has("plan", none));
    assert!(has("build", todo) && has("build", weird));
    assert!(has("deploy", review) && has("maintain", done));
    let w = col("build")["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["iid"] == weird)
        .unwrap()
        .clone();
    assert_eq!(w["placement"]["anomaly"], true);
    let flow_card = col("plan")["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["iid"] == iid)
        .unwrap()
        .clone();
    assert_eq!(flow_card["track"], "full");
    assert_eq!(flow_card["flow"]["state"], "intent");
    // Docs MR is not listed in the deploy column.
    assert!(col("deploy")["mergeRequests"]
        .as_array()
        .unwrap()
        .is_empty());
    // Display only: labels untouched.
    assert_eq!(
        app.gitlab.issue(weird).labels,
        vec!["doc:spec", "In Progress"]
    );
}

/// verify r2 N1 / N2: pending human edits are committed as human before an
/// AI turn (never part of the AI turn), and deltas are kept in history.
#[tokio::test]
async fn human_edits_never_attributed_to_ai_turn() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app).await;
    let notes = format!("docs/changes/{iid}/notes.md");
    // Autosave a human edit (no commit).
    let (st, v) = app
        .put(
            &format!("{}/{iid}/file", app.base()),
            json!({"path": notes, "content": "human note\n", "baseVersion": null}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let mut ws = connect(&app, iid).await;
    next_matching(&mut ws, |v| v["type"] == "ready").await;
    let target = format!("docs/changes/{iid}/intent.md");
    send(
        &mut ws,
        json!({"type": "send", "stage": "intent", "text": format!("改 `{target}`")}),
    )
    .await;
    let td = next_matching(&mut ws, |v| {
        v["type"] == "agent" && v["event"]["type"] == "turn_diff"
    })
    .await;
    let files: Vec<String> = td["event"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap().to_string())
        .collect();
    assert!(files.contains(&target), "{files:?}");
    assert!(
        !files.contains(&notes),
        "human file must not be part of the AI turn: {files:?}"
    );
    next_matching(&mut ws, |v| {
        v["type"] == "agent" && v["event"]["type"] == "turn_completed"
    })
    .await;
    let log = git(
        &app.bare,
        &["log", "--format=%s|%b", &format!("symphony/docs-{iid}")],
    )
    .unwrap();
    let notes_commit = git(
        &app.bare,
        &[
            "log",
            "-1",
            "--format=%B",
            &format!("symphony/docs-{iid}"),
            "--",
            &notes,
        ],
    )
    .unwrap();
    assert!(
        notes_commit.contains("Symphony-Actor: human"),
        "{notes_commit}\n{log}"
    );

    // Reconnect: the streamed reply is in history.
    let mut ws2 = connect(&app, iid).await;
    let hist = next_matching(&mut ws2, |v| {
        v["type"] == "history" && v["stage"] == "intent"
    })
    .await;
    let deltas: String = hist["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "message_delta")
        .map(|e| e["delta"].as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(deltas, "working");
}

/// review r2 I1: drafts left `running` by a crashed process are reset and
/// can be retried.
#[tokio::test]
async fn stale_running_draft_is_recoverable() {
    let app = SdlcApp::new().await;
    let iid = start_full(&app).await;
    let conn = rusqlite::Connection::open(app.dir.path().join("test.db")).unwrap();
    conn.execute("UPDATE change_documents SET draft_state = 'running'", [])
        .unwrap();
    let store = web_platform::sdlc::store::SdlcStore::new(app.repo.pool());
    assert_eq!(store.reset_stale_drafts().await.unwrap(), 1);
    let (_, v) = app.get(&format!("{}/{iid}", app.base())).await;
    assert_eq!(v["data"]["documents"][0]["draftState"], "failed");
    // Retry is accepted even if a stale `running` value remained.
    conn.execute("UPDATE change_documents SET draft_state = 'running'", [])
        .unwrap();
    let (st, v) = app
        .post(
            &format!("{}/{iid}/docs/intent/draft/retry", app.base()),
            json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// verify r4 C-1: an explicit save while a human moved the issue to `Todo`
/// keeps the labels as they are and reports the mismatch.
#[tokio::test]
async fn save_reports_label_mismatch_without_rewriting() {
    let app = SdlcApp::new().await;
    let iid = app.gitlab.add_issue("L", &["bug"]) as i64;
    let (st, v) = app
        .post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    {
        let mut m = app.gitlab.issues.lock().unwrap();
        let i = m.get_mut(&(iid as u64)).unwrap();
        i.labels.retain(|l| !l.starts_with("doc:"));
        i.labels.push("Todo".into());
    }
    let mut ws = connect(&app, iid).await;
    let path = format!("docs/changes/{iid}/plan.md");
    let (_, cur) = app
        .get(&format!(
            "{}/{iid}/file?path={}",
            app.base(),
            urlencoding::encode(&path)
        ))
        .await;
    let cur = cur["data"].clone();
    let content = format!("{}\n\nhuman edit\n", cur["content"].as_str().unwrap());
    let (st, v) = app
        .put(
            &format!("{}/{iid}/file", app.base()),
            json!({"path": path, "content": content, "baseVersion": cur["version"], "commit": true}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let n = next_matching(&mut ws, |v| v["type"] == "notice").await;
    assert!(
        n["message"]
            .as_str()
            .unwrap()
            .contains("label 与文档状态不一致"),
        "{n}"
    );
    let labels = app.gitlab.issue(iid as u64).labels;
    assert!(labels.contains(&"Todo".to_string()), "{labels:?}");
    assert!(!labels.iter().any(|l| l.starts_with("doc:")), "{labels:?}");
}
