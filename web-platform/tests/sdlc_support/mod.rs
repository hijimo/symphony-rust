//! Test harness for the AI-native SDLC flow:
//! - an in-process fake GitLab REST API (issues, labels, notes, MRs, merge),
//! - a local bare git repository acting as the remote,
//! - a fake `codex app-server` (Python script speaking JSON-RPC 2.0) that
//!   edits the target document like a real agent would.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use axum::extract::{Path as AxPath, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use dashmap::DashMap;
use reqwest::Client;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::net::TcpListener;

use web_platform::auth::password::hash_password;
use web_platform::auth::rate_limit::RateLimiter;
use web_platform::concurrency::ConcurrencyManager;
use web_platform::crypto;
use web_platform::db::init_pool;
use web_platform::process_manager::ProcessManager;
use web_platform::repository::{SqliteRepository, UserConfigRepository, UserRepository};
use web_platform::router::create_router;
use web_platform::sdlc::agent::{AgentConfig, AgentManager};
use web_platform::sdlc::platform::PlatformFactory;
use web_platform::sdlc::SdlcState;
use web_platform::services::cache::ApiCache;
use web_platform::{AppState, Phase3RateLimiter, PlatformHostSemaphores};

pub const KEY: [u8; 32] = [0x42u8; 32];
pub const NS: &str = "team";
pub const REPO: &str = "app";

// ---------------------------------------------------------------- fake GitLab

#[derive(Debug, Clone)]
pub struct FakeIssue {
    pub iid: u64,
    pub title: String,
    pub description: Option<String>,
    pub labels: Vec<String>,
    pub state: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FakeMr {
    pub iid: u64,
    pub title: String,
    pub source: String,
    pub target: String,
    pub state: String,
    pub notes: Vec<String>,
    pub merge_sha: Option<String>,
}

#[derive(Default)]
pub struct GitLabState {
    pub issues: Mutex<BTreeMap<u64, FakeIssue>>,
    pub mrs: Mutex<BTreeMap<u64, FakeMr>>,
    /// Bare repo used to perform real merges.
    pub bare: Mutex<Option<PathBuf>>,
    /// access_level returned for GET /projects/:path (30 = developer).
    pub access_level: Mutex<i64>,
    /// When set, the next merge attempt fails with 405 (policy).
    pub fail_merge: Mutex<bool>,
    /// When set, creating a merge request fails with 500.
    pub fail_create_mr: Mutex<bool>,
    /// Reported MR merge_status (default can_be_merged).
    pub mr_merge_status: Mutex<Option<String>>,
    /// Tokens seen on merge calls (to check approver token is used).
    pub merge_tokens: Mutex<Vec<String>>,
}

impl GitLabState {
    pub fn issue(&self, iid: u64) -> FakeIssue {
        self.issues.lock().unwrap().get(&iid).cloned().unwrap()
    }
    pub fn mr(&self, iid: u64) -> FakeMr {
        self.mrs.lock().unwrap().get(&iid).cloned().unwrap()
    }
    pub fn add_issue(&self, title: &str, labels: &[&str]) -> u64 {
        let mut m = self.issues.lock().unwrap();
        let iid = m.keys().max().copied().unwrap_or(0) + 1;
        m.insert(
            iid,
            FakeIssue {
                iid,
                title: title.into(),
                description: None,
                labels: labels.iter().map(|s| s.to_string()).collect(),
                state: "opened".into(),
                notes: vec![],
            },
        );
        iid
    }
}

type GS = Arc<GitLabState>;

fn user_json() -> Value {
    json!({"username": "alice", "name": "Alice", "avatar_url": null})
}

fn issue_json(i: &FakeIssue) -> Value {
    json!({
        "iid": i.iid, "title": i.title, "description": i.description, "state": i.state,
        "labels": i.labels, "author": user_json(), "assignees": [], "milestone": null,
        "created_at": "2026-10-01T00:00:00Z", "updated_at": "2026-10-01T00:00:00Z",
        "closed_at": null, "web_url": format!("http://gitlab.local/{NS}/{REPO}/-/issues/{}", i.iid),
        "user_notes_count": i.notes.len(),
    })
}

fn mr_json(m: &FakeMr) -> Value {
    mr_json_status(m, "can_be_merged")
}

fn mr_json_status(m: &FakeMr, merge_status: &str) -> Value {
    json!({
        "id": m.iid + 1000, "iid": m.iid, "title": m.title, "description": null, "state": m.state,
        "author": user_json(), "source_branch": m.source, "target_branch": m.target,
        "reviewers": [], "merge_status": merge_status,
        "created_at": "2026-10-01T00:00:00Z", "updated_at": "2026-10-01T00:00:00Z",
        "merged_at": null, "web_url": format!("http://gitlab.local/{NS}/{REPO}/-/merge_requests/{}", m.iid),
        "merge_commit_sha": m.merge_sha,
        "references": {"full": format!("{NS}/{REPO}!{}", m.iid)},
    })
}

fn split_labels(v: Option<&Value>) -> Vec<String> {
    v.and_then(|x| x.as_str())
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

async fn get_project(State(st): State<GS>, AxPath(_p): AxPath<String>) -> Json<Value> {
    let level = *st.access_level.lock().unwrap();
    Json(json!({"path_with_namespace": format!("{NS}/{REPO}"),
        "permissions": {"project_access": {"access_level": level}, "group_access": null}}))
}

async fn list_issues(
    State(st): State<GS>,
    AxPath(_p): AxPath<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let want = q.get("state").cloned().unwrap_or_else(|| "opened".into());
    let m = st.issues.lock().unwrap();
    let list: Vec<Value> = m
        .values()
        .filter(|i| i.state == want)
        .map(|i| {
            let mut v = issue_json(i);
            if i.state == "closed" {
                v["closed_at"] = json!(chrono::Utc::now().to_rfc3339());
            }
            v
        })
        .collect();
    Json(Value::Array(list))
}

async fn list_mrs(State(st): State<GS>, AxPath(_p): AxPath<String>) -> Json<Value> {
    let m = st.mrs.lock().unwrap();
    Json(Value::Array(
        m.values()
            .filter(|x| x.state == "opened")
            .map(mr_json)
            .collect(),
    ))
}

async fn create_issue(
    State(st): State<GS>,
    AxPath(_p): AxPath<String>,
    Json(b): Json<Value>,
) -> Json<Value> {
    let iid = st.add_issue(b["title"].as_str().unwrap_or(""), &[]);
    let mut m = st.issues.lock().unwrap();
    let i = m.get_mut(&iid).unwrap();
    i.description = b["description"].as_str().map(str::to_string);
    i.labels = split_labels(b.get("labels"));
    Json(issue_json(i))
}

async fn get_issue(
    State(st): State<GS>,
    AxPath((_p, iid)): AxPath<(String, u64)>,
) -> Result<Json<Value>, StatusCode> {
    st.issues
        .lock()
        .unwrap()
        .get(&iid)
        .map(|i| Json(issue_json(i)))
        .ok_or(StatusCode::NOT_FOUND)
}

async fn update_issue(
    State(st): State<GS>,
    AxPath((_p, iid)): AxPath<(String, u64)>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let mut m = st.issues.lock().unwrap();
    let i = m.get_mut(&iid).ok_or(StatusCode::NOT_FOUND)?;
    for l in split_labels(b.get("remove_labels")) {
        i.labels.retain(|x| *x != l);
    }
    for l in split_labels(b.get("add_labels")) {
        if !i.labels.contains(&l) {
            i.labels.push(l);
        }
    }
    if b["state_event"] == "close" {
        i.state = "closed".into();
    }
    Ok(Json(issue_json(i)))
}

async fn issue_note(
    State(st): State<GS>,
    AxPath((_p, iid)): AxPath<(String, u64)>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let mut m = st.issues.lock().unwrap();
    let i = m.get_mut(&iid).ok_or(StatusCode::NOT_FOUND)?;
    i.notes.push(b["body"].as_str().unwrap_or("").to_string());
    Ok(Json(json!({"id": i.notes.len()})))
}

async fn create_mr(
    State(st): State<GS>,
    AxPath(_p): AxPath<String>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    if *st.fail_create_mr.lock().unwrap() {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let mut m = st.mrs.lock().unwrap();
    let iid = m.keys().max().copied().unwrap_or(0) + 1;
    let mr = FakeMr {
        iid,
        title: b["title"].as_str().unwrap_or("").into(),
        source: b["source_branch"].as_str().unwrap_or("").into(),
        target: b["target_branch"].as_str().unwrap_or("").into(),
        state: "opened".into(),
        notes: vec![],
        merge_sha: None,
    };
    let v = mr_json(&mr);
    m.insert(iid, mr);
    Ok(Json(v))
}

async fn get_mr(
    State(st): State<GS>,
    AxPath((_p, iid)): AxPath<(String, u64)>,
) -> Result<Json<Value>, StatusCode> {
    let ms = st
        .mr_merge_status
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| "can_be_merged".into());
    st.mrs
        .lock()
        .unwrap()
        .get(&iid)
        .map(|m| Json(mr_json_status(m, &ms)))
        .ok_or(StatusCode::NOT_FOUND)
}

async fn update_mr(
    State(st): State<GS>,
    AxPath((_p, iid)): AxPath<(String, u64)>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let mut m = st.mrs.lock().unwrap();
    let mr = m.get_mut(&iid).ok_or(StatusCode::NOT_FOUND)?;
    if let Some(t) = b["title"].as_str() {
        mr.title = t.into();
    }
    if b["state_event"] == "close" {
        mr.state = "closed".into();
    }
    Ok(Json(mr_json(mr)))
}

async fn mr_note(
    State(st): State<GS>,
    AxPath((_p, iid)): AxPath<(String, u64)>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let mut m = st.mrs.lock().unwrap();
    let mr = m.get_mut(&iid).ok_or(StatusCode::NOT_FOUND)?;
    mr.notes.push(b["body"].as_str().unwrap_or("").to_string());
    Ok(Json(json!({"id": mr.notes.len()})))
}

async fn merge_mr(
    State(st): State<GS>,
    headers: axum::http::HeaderMap,
    AxPath((_p, iid)): AxPath<(String, u64)>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let token = headers
        .get("PRIVATE-TOKEN")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    st.merge_tokens.lock().unwrap().push(token);
    assert_eq!(b["squash"], false, "docs MR must not be squashed");
    if std::mem::take(&mut *st.fail_merge.lock().unwrap()) {
        return Err((
            StatusCode::METHOD_NOT_ALLOWED,
            Json(json!({"message": "405 Method Not Allowed"})),
        ));
    }
    let (src, tgt) = {
        let m = st.mrs.lock().unwrap();
        let mr = m
            .get(&iid)
            .ok_or((StatusCode::NOT_FOUND, Json(json!({}))))?;
        if mr.title.starts_with("Draft:") {
            return Err((
                StatusCode::METHOD_NOT_ALLOWED,
                Json(json!({"message": "draft"})),
            ));
        }
        (mr.source.clone(), mr.target.clone())
    };
    let bare = st.bare.lock().unwrap().clone().unwrap();
    let sha = real_merge(&bare, &src, &tgt)
        .map_err(|e| (StatusCode::METHOD_NOT_ALLOWED, Json(json!({"message": e}))))?;
    let mut m = st.mrs.lock().unwrap();
    let mr = m.get_mut(&iid).unwrap();
    mr.state = "merged".into();
    mr.merge_sha = Some(sha.clone());
    Ok(Json(
        json!({"iid": iid, "state": "merged", "merge_commit_sha": sha}),
    ))
}

/// Merge `src` into `tgt` in a bare repo with a real merge commit.
fn real_merge(bare: &Path, src: &str, tgt: &str) -> Result<String, String> {
    let tmp = tempfile::tempdir().unwrap();
    let wt = tmp.path().join("wt");
    git(
        bare,
        &["worktree", "add", "--detach", wt.to_str().unwrap(), tgt],
    )?;
    git(
        &wt,
        &[
            "-c",
            "user.name=GitLab",
            "-c",
            "user.email=gl@local",
            "merge",
            "--no-ff",
            "--no-edit",
            src,
        ],
    )?;
    let sha = git(&wt, &["rev-parse", "HEAD"])?.trim().to_string();
    git(bare, &["update-ref", &format!("refs/heads/{tgt}"), &sha])?;
    git(
        bare,
        &["worktree", "remove", "--force", wt.to_str().unwrap()],
    )?;
    Ok(sha)
}

pub fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    if dir.extension().is_some_and(|e| e == "git") && dir.join("HEAD").exists() {
        cmd.arg("--git-dir").arg(dir);
    }
    let out = cmd
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub async fn spawn_fake_gitlab(st: GS) -> String {
    let app = Router::new()
        .route("/api/v4/projects/{p}", get(get_project))
        .route(
            "/api/v4/projects/{p}/issues",
            post(create_issue).get(list_issues),
        )
        .route(
            "/api/v4/projects/{p}/issues/{iid}",
            get(get_issue).put(update_issue),
        )
        .route("/api/v4/projects/{p}/issues/{iid}/notes", post(issue_note))
        .route(
            "/api/v4/projects/{p}/merge_requests",
            post(create_mr).get(list_mrs),
        )
        .route(
            "/api/v4/projects/{p}/merge_requests/{iid}",
            get(get_mr).put(update_mr),
        )
        .route(
            "/api/v4/projects/{p}/merge_requests/{iid}/notes",
            post(mr_note),
        )
        .route(
            "/api/v4/projects/{p}/merge_requests/{iid}/merge",
            put(merge_mr),
        )
        .route(
            "/api/v4/projects/{p}/merge_requests/{iid}/closes_issues",
            get(|| async { Json(json!([])) }),
        )
        .with_state(st);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("http://{addr}")
}

/// Factory pointing clones at local bare repos (by slug) and the API at the fake.
pub struct LocalFactory {
    pub repos: DashMap<String, PathBuf>,
}

#[async_trait::async_trait]
impl PlatformFactory for LocalFactory {
    async fn client(
        &self,
        project: &web_platform::models::Project,
        proxy: &web_platform::proxy::EffectiveProxyConfig,
    ) -> Result<
        Arc<dyn web_platform::services::git_platform::GitPlatformClient>,
        web_platform::error::WebPlatformError,
    > {
        web_platform::sdlc::platform::DefaultPlatformFactory
            .client(project, proxy)
            .await
    }
    fn clone_url(&self, _project: &web_platform::models::Project, slug: &str) -> String {
        self.repos
            .get(slug)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("/nonexistent/{slug}"))
    }
}

/// Create a bare repo with an initial commit on `main`.
pub fn make_bare(root: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
    let bare = root.join(format!("{name}.git"));
    git(
        root,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    )
    .unwrap();
    let seed = root.join(format!("{name}-seed"));
    git(root, &["init", "-q", "-b", "main", seed.to_str().unwrap()]).unwrap();
    for (p, c) in files {
        let f = seed.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    git(&seed, &["add", "-A"]).unwrap();
    git(
        &seed,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "init",
        ],
    )
    .unwrap();
    git(&seed, &["push", "-q", bare.to_str().unwrap(), "main"]).unwrap();
    bare
}

pub fn read_branch_file(bare: &Path, branch: &str, path: &str) -> Option<String> {
    git(bare, &["show", &format!("{branch}:{path}")]).ok()
}

// ----------------------------------------------------------- fake codex

/// Python fake of `codex app-server`. Behaviour per turn is chosen by the
/// prompt text:
///   contains "[outside]" -> also tries to write README.md (must be declined / discarded)
///   contains "[fail]"    -> turn completes with status failed
///   contains "[ask]"     -> asks for approval of `rm -rf /tmp/x` and records the decision
///   otherwise            -> writes a complete document for the target file
pub const FAKE_CODEX: &str = r###"
import json, sys, os, re
thread = None
n = 0
def send(o):
    sys.stdout.write(json.dumps(o) + "\n"); sys.stdout.flush()
pending = {}
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
DOCS = {
 "intent": "# Intent\n\n## Problem\nusers cannot X\n\n## Proposed outcome\nthey can X\n\n## Affected users and systems\nportal\n\n## Constraints\nnone\n\n## Open questions\nnone\n",
 "spec": "# Spec\n\n## Requirements\nR1\n\n## Design\nD1\n\n## Concerns\n- PII. owner: sec. resolution: masked\n",
 "plan": "# Plan\n\n## Files\n- src/new.rs\n\n## Steps\n1. do\n\n## Tests\n- t\n\n## Risks\n- r\n\n## Alternatives considered\n- none\n",
}
def keep_fm(path, body):
    fm = ""
    if os.path.exists(path):
        old = open(path).read()
        if old.startswith("---\n"):
            end = old.find("\n---\n", 4)
            if end >= 0: fm = old[:end+5]
    return fm + body
while True:
    line = sys.stdin.readline()
    if not line: break
    msg = json.loads(line)
    method = msg.get("method"); mid = msg.get("id"); p = msg.get("params") or {}
    if method == "initialize":
        send({"jsonrpc":"2.0","id":mid,"result":{"userAgent":"fake"}})
    elif method == "initialized":
        pass
    elif method in ("thread/start", "thread/resume"):
        thread = p.get("threadId") or "thread-fake-1"
        open(os.environ.get("FAKE_CODEX_LOG","/tmp/fake-codex.log"), "a").write(method + " " + thread + "\n")
        send({"jsonrpc":"2.0","id":mid,"result":{"thread":{"id":thread}}})
    elif method == "turn/start":
        n += 1
        turn = "turn-%d" % n
        text = "".join(i.get("text","") for i in p.get("input", []))
        send({"jsonrpc":"2.0","id":mid,"result":{"turn":{"id":turn,"status":"inProgress"}}})
        send({"jsonrpc":"2.0","method":"turn/started","params":{"threadId":thread,"turn":{"id":turn}}})
        send({"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"itemId":"m1","threadId":thread,"turnId":turn,"delta":"working"}})
        if "[fail]" in text:
            send({"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":thread,"turn":{"id":turn,"status":"failed","error":{"message":"boom"}}}})
            continue
        if "[ask]" in text:
            r = request("item/commandExecution/requestApproval", {"itemId":"c1","threadId":thread,"turnId":turn,"startedAtMs":0,"command":"rm -rf /tmp/x","commandActions":[{"type":"unknown","command":"rm"}]})
            open(os.environ.get("FAKE_CODEX_LOG","/tmp/fake-codex.log"), "a").write(json.dumps(r) + "\n")
        m = re.search(r"`([^`]*docs/changes/\d+)/(intent|spec|plan)\.md`", text)
        if m:
            rel = m.group(1) + "/" + m.group(2) + ".md"
            path = os.path.join(os.getcwd(), rel)
            ch = [{"path": path, "kind": {"type":"update"}, "diff": ""}]
            send({"jsonrpc":"2.0","method":"item/started","params":{"threadId":thread,"turnId":turn,"startedAtMs":0,"item":{"type":"fileChange","id":"f1","status":"inProgress","changes":ch}}})
            r = request("item/fileChange/requestApproval", {"itemId":"f1","threadId":thread,"turnId":turn,"startedAtMs":0})
            open(os.environ.get("FAKE_CODEX_LOG","/tmp/fake-codex.log"), "a").write("file " + rel + " " + json.dumps(r) + "\n")
            if r and r.get("decision") == "accept":
                body = DOCS[m.group(2)]
                if "Feedback:" in text:
                    body = body.replace("\n## ", "\n<!-- revised -->\n## ", 1)
                open(path, "w").write(keep_fm(path, body))
            send({"jsonrpc":"2.0","method":"item/completed","params":{"threadId":thread,"turnId":turn,"completedAtMs":0,"item":{"type":"fileChange","id":"f1","status":"completed","changes":ch}}})
        if "[outside]" in text:
            outside = os.path.join(os.getcwd(), "README.md")
            ch = [{"path": outside, "kind": {"type":"update"}, "diff": ""}]
            send({"jsonrpc":"2.0","method":"item/started","params":{"threadId":thread,"turnId":turn,"startedAtMs":0,"item":{"type":"fileChange","id":"f2","status":"inProgress","changes":ch}}})
            r = request("item/fileChange/requestApproval", {"itemId":"f2","threadId":thread,"turnId":turn,"startedAtMs":0})
            open(os.environ.get("FAKE_CODEX_LOG","/tmp/fake-codex.log"), "a").write(json.dumps(r) + "\n")
            # misbehave: write anyway to exercise the git-status fallback
            open(outside, "w").write("hacked\n")
        send({"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":thread,"turn":{"id":turn,"status":"completed"}}})
    elif mid is not None:
        send({"jsonrpc":"2.0","id":mid,"result":{}})
"###;

// ----------------------------------------------------------- test app

pub struct SdlcApp {
    pub addr: String,
    pub client: Client,
    pub token: String,
    pub repo: SqliteRepository,
    pub gitlab: GS,
    pub gitlab_url: String,
    pub project_id: i64,
    pub bare: PathBuf,
    pub backend_bare: Option<PathBuf>,
    pub dir: TempDir,
    pub workspace_root: PathBuf,
}

pub struct Opts {
    pub multi_repo: bool,
    pub codex_script: Option<String>,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            multi_repo: false,
            codex_script: Some(FAKE_CODEX.to_string()),
        }
    }
}

impl SdlcApp {
    pub async fn new() -> Self {
        Self::with(Opts::default()).await
    }

    pub async fn with(opts: Opts) -> Self {
        let dir = TempDir::new().unwrap();
        let gitlab = Arc::new(GitLabState::default());
        *gitlab.access_level.lock().unwrap() = 40;
        let gitlab_url = spawn_fake_gitlab(gitlab.clone()).await;

        let bare = make_bare(
            dir.path(),
            "app",
            &[
                ("README.md", "# app\n"),
                ("src/main.rs", "fn main() {}\n"),
                (".gitignore", "target/\n"),
            ],
        );
        *gitlab.bare.lock().unwrap() = Some(bare.clone());
        let factory = LocalFactory {
            repos: DashMap::new(),
        };
        factory.repos.insert(format!("{NS}/{REPO}"), bare.clone());
        let backend_bare = if opts.multi_repo {
            let b = make_bare(
                dir.path(),
                "backend",
                &[("src/api.rs", "pub fn api() {}\n")],
            );
            factory.repos.insert(format!("{NS}/backend"), b.clone());
            Some(b)
        } else {
            None
        };

        let db_path = dir.path().join("test.db");
        let pool = init_pool(db_path.to_str().unwrap());
        let repo = SqliteRepository::new(pool);
        let hash = hash_password("admin123").unwrap();
        let admin = repo
            .create_user("admin", &hash, Some("Administrator"), "admin")
            .await
            .unwrap();
        let tok = crypto::encrypt("admin-gitlab-token", &KEY).unwrap();
        repo.upsert_config(admin.id, Some(&tok), Some(&gitlab_url), None, None, None)
            .await
            .unwrap();

        let script = dir.path().join("fake_codex.py");
        let command = match &opts.codex_script {
            Some(s) => {
                std::fs::write(&script, s).unwrap();
                format!(
                    "FAKE_CODEX_LOG={} python3 {}",
                    dir.path().join("codex.log").display(),
                    script.display()
                )
            }
            None => "false".to_string(),
        };
        let agents = AgentManager::new(
            AgentConfig {
                command,
                env_passthrough: vec![],
                idle_timeout: std::time::Duration::from_secs(600),
                turn_timeout: std::time::Duration::from_secs(60),
                stall_timeout: std::time::Duration::from_secs(30),
            },
            4,
        );
        let workspace_root = dir.path().join("ws");
        let sdlc = SdlcState::new(
            repo.pool(),
            workspace_root.to_str().unwrap(),
            agents,
            Arc::new(factory),
        );
        let state = AppState {
            sdlc,
            repo: repo.clone(),
            jwt_secret: "test-jwt-secret-key-at-least-32-characters-long".to_string(),
            encryption_key: KEY,
            token_blacklist: Arc::new(DashMap::new()),
            rate_limiter: Arc::new(RateLimiter::new()),
            process_manager: ProcessManager::new(),
            api_cache: Arc::new(ApiCache::new(10, 3, 10000)),
            ai_service: None,
            phase3_rate_limiter: Arc::new(Phase3RateLimiter::new()),
            concurrency_manager: Arc::new(ConcurrencyManager::new(10)),
            symphony_bin: "/usr/bin/false".into(),
            workspace_root: workspace_root.to_string_lossy().to_string(),
            alert_manager: None,
            platform_host_semaphores: Arc::new(PlatformHostSemaphores::new(5)),
        };
        let app = create_router(state).into_make_service_with_connect_info::<SocketAddr>();
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });

        let client = Client::new();
        let login: Value = client
            .post(format!("{addr}/api/auth/login"))
            .json(&json!({"username": "admin", "password": "admin123"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let token = login["data"]["token"].as_str().unwrap().to_string();

        let created: Value = client
            .post(format!("{addr}/api/projects"))
            .bearer_auth(&token)
            .json(&json!({"git_url": format!("{gitlab_url}/{NS}/{REPO}")}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(created["success"], true, "{created}");
        let project_id = created["data"]["id"].as_i64().unwrap();

        if opts.multi_repo {
            // Read-only related repo (raw source on the same instance).
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute(
                "INSERT INTO project_repos (project_id, alias, namespace, repo_name, default_branch, mode, created_by) VALUES (?1, 'backend', ?2, 'backend', 'main', 'read', ?3)",
                rusqlite::params![project_id, NS, admin.id],
            ).unwrap();
        }

        Self {
            addr,
            client,
            token,
            repo,
            gitlab,
            gitlab_url,
            project_id,
            bare,
            backend_bare,
            dir,
            workspace_root,
        }
    }

    pub fn url(&self, p: &str) -> String {
        format!("{}{}", self.addr, p)
    }

    pub fn base(&self) -> String {
        format!("/api/projects/{}/changes", self.project_id)
    }

    pub async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = self
            .client
            .request(method, self.url(path))
            .bearer_auth(&self.token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await.unwrap();
        let st = StatusCode::from_u16(resp.status().as_u16()).unwrap();
        let v: Value = resp.json().await.unwrap_or(Value::Null);
        (st, v)
    }

    pub async fn get(&self, path: &str) -> (StatusCode, Value) {
        self.call(reqwest::Method::GET, path, None).await
    }
    pub async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(reqwest::Method::POST, path, Some(body)).await
    }
    pub async fn put(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(reqwest::Method::PUT, path, Some(body)).await
    }

    /// Poll the flow until `pred` holds (AI drafting runs in background).
    pub async fn wait_flow(&self, iid: i64, pred: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..200 {
            let (_, v) = self.get(&format!("{}/{iid}", self.base())).await;
            if pred(&v["data"]) {
                return v["data"].clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let (_, v) = self.get(&format!("{}/{iid}", self.base())).await;
        let ws = self.primary_ws(iid);
        let decisions = std::fs::read_to_string(self.codex_log()).unwrap_or_default();
        let spec = std::fs::read_to_string(ws.join(format!("docs/changes/{iid}/spec.md")))
            .unwrap_or_default();
        panic!("flow condition not reached: {v:#}\nthreads={decisions}\nspec={spec}");
    }

    /// Platform label sync runs after the DB state change (background drafts
    /// auto-submit, then sync labels): poll until the label is present.
    pub async fn wait_label(&self, iid: i64, label: &str) -> Vec<String> {
        for _ in 0..200 {
            let labels = self.gitlab.issue(iid as u64).labels;
            if labels.iter().any(|l| l == label) {
                return labels;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!(
            "label {label} not set on #{iid}: {:?}",
            self.gitlab.issue(iid as u64).labels
        );
    }

    /// Wait for an issue note containing `needle` (comments are posted after
    /// the state change becomes visible).
    pub async fn wait_note(&self, iid: i64, needle: &str) {
        for _ in 0..200 {
            if self
                .gitlab
                .issue(iid as u64)
                .notes
                .iter()
                .any(|n| n.contains(needle))
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!(
            "note {needle:?} not posted on #{iid}: {:?}",
            self.gitlab.issue(iid as u64).notes
        );
    }

    pub fn doc_status(flow: &Value, doc: &str) -> String {
        flow["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["docType"] == doc)
            .map(|d| d["status"].as_str().unwrap().to_string())
            .unwrap_or_default()
    }

    pub fn codex_log(&self) -> PathBuf {
        self.dir.path().join("codex.log")
    }

    pub fn primary_ws(&self, iid: i64) -> PathBuf {
        self.workspace_root
            .join("editor")
            .join(self.project_id.to_string())
            .join(iid.to_string())
    }
}

pub const INTENT_OK: &str = "# Intent\n\n## Problem\nusers cannot see claim status\n\n## Proposed outcome\nportal shows status\n\n## Affected users and systems\nportal\n\n## Constraints\nno new PII\n\n## Open questions\nadjusters?\n";
