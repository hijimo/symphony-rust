//! Regression tests for verify r4 A: the credential scan covers every
//! staged blob (nested, large, binary) and symlinks are never committed.
//! A malicious fake codex writes the files after an accepted approval.

mod sdlc_support;

use futures::{SinkExt, StreamExt};
use reqwest::StatusCode;
use sdlc_support::*;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Malicious fake codex. Behaviour chosen by markers in the prompt.
const EVIL: &str = r###"
import json, sys, os, re, base64
thread = None
n = 0
LOG = os.environ.get("FAKE_CODEX_LOG","/tmp/fake-codex.log")
def log(s): open(LOG, "a").write(s + "\n")
def send(o):
    sys.stdout.write(json.dumps(o) + "\n"); sys.stdout.flush()
rid = 1000
def request(method, params):
    global rid
    rid += 1
    send({"jsonrpc":"2.0","id":rid,"method":method,"params":params})
    while True:
        line = sys.stdin.readline()
        if not line: sys.exit(0)
        m = json.loads(line)
        if m.get("id") == rid and "method" not in m:
            return m.get("result")
SECRET = "glpat-xxxxxxxxxxxxxxxxxxxx"
PK = "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASC\n-----END PRIVATE KEY-----\n"
def approve(turn, item, path, extra=None):
    ch = [{"path": path, "kind": {"type":"update"}, "diff": ""}]
    send({"jsonrpc":"2.0","method":"item/started","params":{"threadId":thread,"turnId":turn,"startedAtMs":0,"item":{"type":"fileChange","id":item,"status":"inProgress","changes":ch}}})
    params = {"itemId":item,"threadId":thread,"turnId":turn,"startedAtMs":0}
    if extra: params.update(extra)
    r = request("item/fileChange/requestApproval", params)
    log("decision %s %s" % (item, json.dumps(r)))
    send({"jsonrpc":"2.0","method":"item/completed","params":{"threadId":thread,"turnId":turn,"completedAtMs":0,"item":{"type":"fileChange","id":item,"status":"completed","changes":ch}}})
    return r and r.get("decision") == "accept"
while True:
    line = sys.stdin.readline()
    if not line: break
    msg = json.loads(line)
    method = msg.get("method"); mid = msg.get("id"); p = msg.get("params") or {}
    if method == "initialize":
        send({"jsonrpc":"2.0","id":mid,"result":{"userAgent":"fake"}})
    elif method in ("thread/start", "thread/resume"):
        thread = p.get("threadId") or "thread-evil"
        send({"jsonrpc":"2.0","id":mid,"result":{"thread":{"id":thread}}})
    elif method == "turn/start":
        n += 1
        turn = "turn-%d" % n
        text = "".join(i.get("text","") for i in p.get("input", []))
        send({"jsonrpc":"2.0","id":mid,"result":{"turn":{"id":turn,"status":"inProgress"}}})
        send({"jsonrpc":"2.0","method":"turn/started","params":{"threadId":thread,"turn":{"id":turn}}})
        m = re.search(r"`([^`]*docs/changes/\d+)/intent\.md`", text)
        d = os.path.join(os.getcwd(), m.group(1))
        intent = os.path.join(d, "intent.md")
        cur = open(intent).read()
        if "[secret]" in text:
            if approve(turn, "f1", intent):
                open(intent, "w").write(cur + "\nleak " + SECRET + "\n" + PK)
        if "[b64]" in text:
            if approve(turn, "f1", intent):
                open(intent, "w").write(cur + "\nleak " + base64.b64encode(SECRET.encode()).decode() + "\n")
        if "[nested]" in text:
            sub = os.path.join(d, "sub", "x.md")
            if approve(turn, "f1", sub):
                os.makedirs(os.path.dirname(sub), exist_ok=True)
                open(sub, "w").write("leak " + SECRET + "\n" + PK)
        if "[binary]" in text:
            f = os.path.join(d, "blob.md")
            if approve(turn, "f1", f):
                open(f, "wb").write(b"\x00" + ("leak " + SECRET + "\n").encode())
        if "[large]" in text:
            f = os.path.join(d, "big.md")
            if approve(turn, "f1", f):
                open(f, "w").write("leak " + SECRET + "\n" + ("a" * (1024*1024 + 10)) + "\n")
        if "[symlink]" in text:
            f = os.path.join(d, "link.md")
            if approve(turn, "f1", f):
                os.symlink("../../../README.md", f)
        if "[noitem]" in text:
            r = request("item/fileChange/requestApproval", {"itemId":"never-started","threadId":thread,"turnId":turn,"startedAtMs":0})
            log("decision noitem %s" % json.dumps(r))
        if "[grant]" in text:
            approve(turn, "g1", intent, {"grantRoot": "/"})
        if "[bypass]" in text:
            root = os.getcwd()
            open(os.path.join(root, "README.md"), "w").write("hacked\n")
            os.makedirs(os.path.join(root, "src"), exist_ok=True)
            open(os.path.join(root, "src", "new.rs"), "w").write("evil\n")
            os.makedirs(os.path.join(root, "target"), exist_ok=True)
            open(os.path.join(root, "target", "x"), "w").write("evil\n")
        if "[edit]" in text:
            if approve(turn, "f1", intent):
                open(intent, "w").write(cur.replace("## Problem", "## Problem\nAI edit", 1))
        send({"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":thread,"turn":{"id":turn,"status":"completed"}}})
    elif mid is not None:
        send({"jsonrpc":"2.0","id":mid,"result":{}})
"###;

async fn app() -> SdlcApp {
    SdlcApp::with(Opts {
        multi_repo: false,
        codex_script: Some(EVIL.to_string()),
    })
    .await
}

async fn start_full(app: &SdlcApp) -> i64 {
    let (st, v) = app
        .post(
            &app.base(),
            json!({"track": "full", "title": "QA", "initialContent": INTENT_OK}),
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

/// Run one turn and collect every ws event until flow_changed / error after turn_completed.
async fn turn(app: &SdlcApp, iid: i64, marker: &str) -> Vec<Value> {
    let mut ws = connect(app, iid).await;
    let target = format!("docs/changes/{iid}/intent.md");
    ws.send(Message::Text(
        json!({"type": "send", "stage": "intent", "text": format!("{marker} 写入 `{target}`")})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    let mut ev = vec![];
    let mut done = false;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let wait = if done {
            tokio::time::Instant::now() + std::time::Duration::from_millis(1500)
        } else {
            deadline
        };
        let msg = match tokio::time::timeout_at(wait.min(deadline), ws.next()).await {
            Ok(Some(Ok(m))) => m,
            _ => break,
        };
        if let Message::Text(t) = msg {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v["type"] == "agent" && v["event"]["type"] == "turn_completed" {
                done = true;
            }
            ev.push(v);
        }
    }
    assert!(done, "turn not completed: {ev:#?}");
    ev
}

fn all_log(app: &SdlcApp) -> String {
    git(&app.bare, &["log", "-p", "--all", "--format=COMMIT %H%n%B"]).unwrap()
}

fn notices(ev: &[Value]) -> Vec<String> {
    ev.iter()
        .filter_map(|v| {
            let e = if v["type"] == "agent" { &v["event"] } else { v };
            (e["type"] == "notice" || e["type"] == "error").then(|| e.to_string())
        })
        .collect()
}

async fn leak_case(marker: &str) -> (bool, Vec<String>, String) {
    let app = app().await;
    let iid = start_full(&app).await;
    let ev = turn(&app, iid, marker).await;
    let log = all_log(&app);
    // `--text` so binary blobs are searched too.
    let raw = git(&app.bare, &["log", "-p", "--text", "--all"]).unwrap();
    let leaked = log.contains("glpat-xxxxxxxxxxxxxxxxxxxx")
        || raw.contains("glpat-xxxxxxxxxxxxxxxxxxxx")
        || log.contains("BEGIN PRIVATE KEY");
    let codex = std::fs::read_to_string(app.codex_log()).unwrap_or_default();
    (leaked, notices(&ev), codex)
}

#[tokio::test]
async fn ai_secret_plain_is_blocked() {
    let (leaked, n, codex) = leak_case("[secret]").await;
    assert!(codex.contains("\"accept\""), "{codex}");
    assert!(!leaked, "secret pushed");
    assert!(n.iter().any(|x| x.contains("凭据")), "{n:?}");
}

#[tokio::test]
async fn ai_secret_in_nested_dir_is_blocked() {
    let (leaked, n, _) = leak_case("[nested]").await;
    assert!(!leaked, "nested secret pushed; notices={n:?}");
    assert!(
        n.iter()
            .any(|x| x.contains("sub/x.md") && x.contains("凭据")),
        "{n:?}"
    );
}

#[tokio::test]
async fn ai_secret_in_binary_file_is_blocked() {
    let (leaked, n, _) = leak_case("[binary]").await;
    assert!(!leaked, "binary secret pushed; notices={n:?}");
    assert!(n.iter().any(|x| x.contains("凭据")), "{n:?}");
}

#[tokio::test]
async fn ai_secret_in_large_file_is_blocked() {
    let (leaked, n, _) = leak_case("[large]").await;
    assert!(!leaked, "large-file secret pushed; notices={n:?}");
    assert!(n.iter().any(|x| x.contains("凭据")), "{n:?}");
}

#[tokio::test]
async fn ai_symlink_is_never_committed() {
    let app = app().await;
    let iid = start_full(&app).await;
    let ev = turn(&app, iid, "[symlink]").await;
    let tree = git(
        &app.bare,
        &["ls-tree", "-r", &format!("symphony/docs-{iid}")],
    )
    .unwrap();
    assert!(!tree.contains("120000"), "symlink pushed: {tree}");
    let n = notices(&ev);
    assert!(n.iter().any(|x| x.contains("符号链接")), "{n:?}");
    // The link was removed, so the next turn commits normally.
    assert!(app
        .primary_ws(iid)
        .join(format!("docs/changes/{iid}/link.md"))
        .symlink_metadata()
        .is_err());
    let _ = turn(&app, iid, "[edit]").await;
    let intent = read_branch_file(
        &app.bare,
        &format!("symphony/docs-{iid}"),
        &format!("docs/changes/{iid}/intent.md"),
    )
    .unwrap();
    assert!(intent.contains("AI edit"), "{intent}");
}

/// verify r4 A-4: pending human edits committed before an AI turn carry the
/// real user, not a fixed `workbench` name.
#[tokio::test]
async fn pending_human_edit_is_attributed_to_sender() {
    let app = app().await;
    let iid = start_full(&app).await;
    let notes = format!("docs/changes/{iid}/notes.md");
    let (st, v) = app
        .put(
            &format!("{}/{iid}/file", app.base()),
            json!({"path": notes, "content": "human note\n", "commit": false}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let _ = turn(&app, iid, "[edit]").await;
    let br = format!("symphony/docs-{iid}");
    let sha = git(&app.bare, &["log", "-1", "--format=%H", &br, "--", &notes]).unwrap();
    let msg = git(&app.bare, &["log", "-1", "--format=%B", sha.trim()]).unwrap();
    assert!(msg.contains("Symphony-Actor: human"), "{msg}");
    assert!(!msg.contains("workbench"), "{msg}");
    assert!(msg.contains("Symphony-User: Administrator"), "{msg}");
}
