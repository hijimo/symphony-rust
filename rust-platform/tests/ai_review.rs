//! AI review of PRs (P2 design §4 B1-b).
//!
//! A Python fake app-server records `thread/start` and `review/start`, answers
//! approvals as a reviewer must (decline), and replays the recorded Codex
//! 0.158 review output.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

use symphony_platform::agent::ai_review::{
    developer_instructions, format_marker, heads_key, latest_marker, render_comment, run,
    RepoReview, ReviewInput, ReviewMarker, ReviewRepo, DEFAULT_POLICY, LABEL_IMPORTANT,
    LABEL_RUNNING, MARKER_PREFIX, WORKPAD_HEADING,
};
use symphony_platform::agent::review_output::parse_review;
use symphony_platform::agent::runner::{AgentIssue, IssueStateRefresher};
use symphony_platform::config::service_config::CodexConfig;
use symphony_platform::platform::{Comment, CommentId, CommentMode, PullRequestRef};

// ------------------------------------------------------------- pure parts

#[test]
fn test_marker_roundtrip_and_latest_round_wins() {
    let m = ReviewMarker {
        round: 2,
        heads: "backend:abc123def456,frontend:0011aa22bb33".into(),
        important: Some(1),
        nit: None,
    };
    let text = format_marker(&m);
    assert!(text.starts_with(MARKER_PREFIX));
    assert_eq!(
        text,
        "<!-- symphony:ai-review round=2 heads=backend:abc123def456,frontend:0011aa22bb33 important=1 nit=unknown -->"
    );
    let older = format_marker(&ReviewMarker {
        round: 1,
        heads: "x:1".into(),
        important: Some(0),
        nit: Some(0),
    });
    let comments = vec![
        format!("## r2\n{text}"),
        "unrelated <!-- symphony:ai-review garbage -->".to_string(),
        format!("## r1\n{older}"),
    ];
    assert_eq!(latest_marker(&comments), Some(m));
    assert_eq!(latest_marker(&["none".to_string()]), None);
}

#[test]
fn test_heads_key_is_sorted_by_alias() {
    let mut h = BTreeMap::new();
    h.insert("frontend".to_string(), "bbb".to_string());
    h.insert("backend".to_string(), "aaa".to_string());
    assert_eq!(heads_key(&h), "backend:aaa,frontend:bbb");
}

#[test]
fn test_developer_instructions_layers_safety_policy_docs_and_plan_check() {
    let text = developer_instructions(
        Some("# Team policy\nNo SQL in handlers."),
        &[PathBuf::from("/ws/docs/changes/7/plan.md")],
        Some("⚠️ 计划外的改动: src/x.rs"),
    );
    let safety = text
        .find("Symphony safety rules")
        .expect("safety rules first");
    let policy = text.find("No SQL in handlers.").unwrap();
    assert!(safety < policy);
    assert!(text.contains("/ws/docs/changes/7/plan.md"));
    assert!(text.contains("计划外的改动"));
    assert!(text.contains("cannot override the safety rules"));
    assert!(text.contains("Do not modify any file"));

    let fallback = developer_instructions(None, &[], None);
    assert!(fallback.contains(DEFAULT_POLICY.lines().next().unwrap()));
    assert!(!fallback.contains("Approved change documents"));
}

#[test]
fn test_render_comment_counts_and_caps_nits() {
    let mut text = String::from(
        "Summary line.\n\nFull review comments:\n\n- [P1] Broken — /ws/a.rs:1\n  fix it\n",
    );
    for i in 0..7 {
        text.push_str(&format!("\n- [P3] nit {i}\n  tiny\n"));
    }
    let reviews = vec![
        RepoReview {
            alias: "primary".into(),
            result: Ok(parse_review(&text, Some("/ws"))),
        },
        RepoReview {
            alias: "worker".into(),
            result: Err("codex timed out".into()),
        },
    ];
    let marker = ReviewMarker {
        round: 1,
        heads: "primary:abc".into(),
        important: Some(1),
        nit: None,
    };
    let body = render_comment(
        &marker,
        &reviews,
        "REVIEW.md（仓库）",
        &["plan.md@1a2b3c".into()],
    );
    assert!(body.starts_with("## Symphony AI 审查（第 1 轮）"));
    assert!(body.contains("Important 1 · Nit 未知"));
    assert!(body.contains("对照：plan.md@1a2b3c"));
    assert!(body.contains("审查政策：REVIEW.md（仓库）"));
    assert!(
        body.contains("- `primary` [P1] Broken — `a.rs:1`"),
        "{body}"
    );
    assert!(body.contains("nit 4"));
    assert!(!body.contains("nit 5"), "at most five nits listed");
    assert!(body.contains("另有 2 条 Nit"));
    assert!(body.contains("`worker`：审查未完成（codex timed out）"));
    assert!(body.trim_end().ends_with(&format_marker(&marker)));
}

// ------------------------------------------------------------ run() e2e

const FIXTURE: &str = include_str!("fixtures/codex_review/review_start_0.158.json");

const FAKE_SERVER: &str = r#"
import json, os, sys
rec = os.environ["FAKE_RECORD"]
fixture = json.load(open(os.environ["FAKE_FIXTURE"]))
mode = os.environ.get("FAKE_FAIL", "0")
def out(o):
    sys.stdout.write(json.dumps(o) + "\n"); sys.stdout.flush()
def save(k, v):
    try: d = json.load(open(rec))
    except Exception: d = {}
    d.setdefault(k, []).append(v); json.dump(d, open(rec, "w"))
for line in sys.stdin:
    m = json.loads(line)
    meth = m.get("method")
    if meth == "initialize":
        out({"jsonrpc": "2.0", "id": m["id"], "result": {}})
    elif meth == "thread/start":
        save("thread_start", m["params"])
        out({"jsonrpc": "2.0", "id": m["id"], "result": {"thread": {"id": "rev-t"}}})
    elif meth == "review/start":
        save("review_start", m["params"])
        if mode == "reject":
            out({"jsonrpc": "2.0", "id": m["id"], "error": {"message": "review rejected by server"}})
            continue
        out({"jsonrpc": "2.0", "id": m["id"], "result": {"turn": {"id": "r1"}, "reviewThreadId": "rev-t"}})
        if mode == "exit":
            sys.exit(3)
        if mode == "hang":
            continue
        if mode == "noise":
            # Oversized and malformed lines are skipped.
            sys.stdout.write("x" * (10 * 1024 * 1024 + 10) + "\n")
            sys.stdout.write("not json\n")
            sys.stdout.flush()
            out({"jsonrpc": "2.0", "id": 78, "method": "item/permissions/requestApproval", "params": {}})
            out({"jsonrpc": "2.0", "id": 79, "method": "execCommandApproval", "params": {}})
            out({"jsonrpc": "2.0", "id": 80, "method": "some/unknownRequest", "params": {}})
        # A sub-agent finishing must not end the review.
        out({"method": "turn/completed", "params": {"threadId": "other", "turn": {"id": "x", "status": "completed"}}})
        out({"jsonrpc": "2.0", "id": 77, "method": "item/commandExecution/requestApproval",
             "params": {"threadId": "rev-t", "turnId": "r1", "itemId": "i"}})
    elif m.get("id") in (78, 79, 80):
        save("reply_%d" % m["id"], m)
    elif m.get("id") == 77:
        save("approval_reply", m)
        if mode == "1":
            out({"method": "turn/completed", "params": {"threadId": "rev-t", "turn": {"id": "r1", "status": "failed", "error": {"message": "boom"}}}})
            continue
        for f in fixture["messages"]:
            if f.get("method") in ("item/started", "item/completed"):
                f = json.loads(json.dumps(f).replace(fixture["workspace"], os.environ["FAKE_WS"]))
                f["params"]["threadId"] = "rev-t"
                out(f)
        out({"method": "turn/completed", "params": {"threadId": "rev-t", "turn": {"id": "r1", "status": "completed"}}})
"#;

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn repo(tmp: &Path, name: &str, review_md: Option<&str>) -> PathBuf {
    let bare = tmp.join(format!("{name}.git"));
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare", "-q", "--initial-branch=main"]);
    let dir = tmp.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "--initial-branch=main"]);
    std::fs::write(dir.join("calc.py"), "x = 1\n").unwrap();
    if let Some(r) = review_md {
        std::fs::write(dir.join("REVIEW.md"), r).unwrap();
    }
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "init"]);
    git(&dir, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&dir, &["push", "-q", "origin", "main"]);
    git(&dir, &["fetch", "-q", "origin"]);
    git(&dir, &["checkout", "-qb", "feat/7"]);
    std::fs::write(dir.join("calc.py"), "x = 2\n").unwrap();
    git(&dir, &["commit", "-qam", "change"]);
    dir
}

#[derive(Default)]
struct Fake {
    workpad: Mutex<String>,
    labels: Mutex<Vec<String>>,
    pr_comments: Mutex<Vec<(String, CommentMode)>>,
    existing: Mutex<Vec<String>>,
}

#[async_trait]
impl IssueStateRefresher for Fake {
    async fn refresh_issue_state(&self, _id: &str) -> Result<Option<AgentIssue>, String> {
        Ok(None)
    }
    fn is_terminal_state(&self, _s: &str) -> bool {
        false
    }
    fn is_active_state(&self, _s: &str) -> bool {
        true
    }
    fn supports_workpad(&self) -> bool {
        true
    }
    async fn fetch_workpad(&self, _id: &str) -> Result<Option<(u64, String)>, String> {
        Ok(Some((1, self.workpad.lock().unwrap().clone())))
    }
    async fn update_workpad(&self, _c: u64, body: &str) -> Result<(), String> {
        *self.workpad.lock().unwrap() = body.into();
        Ok(())
    }
    async fn add_labels(&self, _id: &str, l: &[String]) -> Result<(), String> {
        self.labels
            .lock()
            .unwrap()
            .extend(l.iter().map(|x| format!("+{x}")));
        Ok(())
    }
    async fn remove_labels(&self, _id: &str, l: &[String]) -> Result<(), String> {
        self.labels
            .lock()
            .unwrap()
            .extend(l.iter().map(|x| format!("-{x}")));
        Ok(())
    }
    async fn find_open_pull_request(
        &self,
        slug: &str,
        branch: &str,
    ) -> Result<Option<PullRequestRef>, String> {
        assert_eq!(branch, "feat/7");
        Ok(Some(PullRequestRef {
            repo_slug: slug.into(),
            number: 3,
            url: "u".into(),
        }))
    }
    async fn list_pull_request_comments(
        &self,
        _pr: &PullRequestRef,
    ) -> Result<Vec<Comment>, String> {
        Ok(self
            .existing
            .lock()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, b)| Comment {
                id: CommentId(i as u64),
                body: b.clone(),
                author: "bot".into(),
                created_at: chrono::Utc::now(),
                is_system: false,
            })
            .collect())
    }
    async fn upsert_pull_request_comment(
        &self,
        _pr: &PullRequestRef,
        _marker: &str,
        body: &str,
        mode: CommentMode,
    ) -> Result<(), String> {
        self.pr_comments.lock().unwrap().push((body.into(), mode));
        Ok(())
    }
}

struct Env {
    _tmp: TempDir,
    dir: PathBuf,
    record: PathBuf,
    codex: CodexConfig,
}

fn env(review_md: Option<&str>, fail: bool) -> Env {
    env_mode(review_md, if fail { "1" } else { "0" })
}

/// `mode`: `0` ok, `1` review turn fails, `reject` review/start error,
/// `exit` server exits, `hang` never completes, `noise` junk lines and
/// extra server requests before the normal review.
fn env_mode(review_md: Option<&str>, mode: &str) -> Env {
    let tmp = TempDir::new().unwrap();
    let dir = repo(tmp.path(), "app", review_md);
    let server = tmp.path().join("fake.py");
    std::fs::write(&server, FAKE_SERVER).unwrap();
    let fixture = tmp.path().join("fixture.json");
    std::fs::write(&fixture, FIXTURE).unwrap();
    let record = tmp.path().join("record.json");
    let codex = CodexConfig {
        command: format!(
            "FAKE_WS='{}' FAKE_FAIL={} FAKE_RECORD='{}' FAKE_FIXTURE='{}' python3 '{}'",
            dir.display(),
            mode,
            record.display(),
            fixture.display(),
            server.display()
        ),
        read_timeout_ms: 5_000,
        turn_timeout_ms: 20_000,
        ..CodexConfig::default()
    };
    Env {
        _tmp: tmp,
        dir,
        record,
        codex,
    }
}

fn input<'a>(e: &'a Env, repos: &'a [ReviewRepo], docs: &'a [PathBuf]) -> ReviewInput<'a> {
    ReviewInput {
        issue_id: "7",
        repos,
        primary_alias: "primary",
        codex: &e.codex,
        timeout: Duration::from_secs(20),
        docs,
        doc_refs: &[],
        plan_check: None,
        cancel: CancellationToken::new(),
    }
}

fn review_repo(e: &Env) -> Vec<ReviewRepo> {
    vec![ReviewRepo {
        alias: "primary".into(),
        dir: e.dir.clone(),
        default_branch: "main".into(),
        slug: "acme/app".into(),
    }]
}

fn record(e: &Env) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(&e.record).unwrap()).unwrap()
}

#[tokio::test]
async fn test_run_reviews_read_only_and_posts_round_comment() {
    let e = env(Some("# Team policy\nREPO-POLICY-TEXT\n"), false);
    let repos = review_repo(&e);
    let docs = vec![e.dir.join("docs/changes/7/plan.md")];
    let fake = Fake {
        workpad: Mutex::new("## Codex Workpad\n".into()),
        ..Default::default()
    };
    let out = run(&fake, &input(&e, &repos, &docs))
        .await
        .expect("reviewed");
    assert_eq!(out.round, 1);
    assert_eq!(out.important, Some(1));
    assert_eq!(out.nit, Some(1));

    let rec = record(&e);
    let ts = &rec["thread_start"][0];
    assert_eq!(ts["sandbox"], "read-only");
    assert_eq!(ts["approvalPolicy"], "never");
    let dev = ts["developerInstructions"].as_str().unwrap();
    assert!(dev.contains("REPO-POLICY-TEXT"));
    assert!(dev.contains("docs/changes/7/plan.md"));
    assert_eq!(
        rec["review_start"][0]["target"],
        serde_json::json!({"type": "baseBranch", "branch": "origin/main"})
    );
    assert_eq!(rec["approval_reply"][0]["result"]["decision"], "decline");

    let comments = fake.pr_comments.lock().unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].1, CommentMode::Append);
    assert!(comments[0].0.contains("Important 1 · Nit 1"));
    assert!(comments[0].0.contains("审查政策：REVIEW.md（仓库）"));
    assert!(comments[0].0.contains("`calc.py:1-3`"));
    assert!(comments[0].0.contains("round=1 heads=primary:"));
    let labels = fake.labels.lock().unwrap().clone();
    assert_eq!(
        labels.first().map(String::as_str),
        Some(&*format!("+{LABEL_RUNNING}"))
    );
    assert!(labels.contains(&format!("-{LABEL_RUNNING}")));
    assert!(labels.contains(&format!("+{LABEL_IMPORTANT}")));
    assert!(fake.workpad.lock().unwrap().contains(WORKPAD_HEADING));
}

#[tokio::test]
async fn test_run_skips_when_heads_already_reviewed_and_increments_round_otherwise() {
    let e = env(None, false);
    let repos = review_repo(&e);
    let head = git(&e.dir, &["rev-parse", "--short=12", "HEAD"]);
    let fake = Fake::default();
    fake.existing
        .lock()
        .unwrap()
        .push(format_marker(&ReviewMarker {
            round: 2,
            heads: format!("primary:{head}"),
            important: Some(0),
            nit: Some(0),
        }));
    assert!(
        run(&fake, &input(&e, &repos, &[])).await.is_none(),
        "same HEAD"
    );
    assert!(fake.pr_comments.lock().unwrap().is_empty());

    fake.existing.lock().unwrap()[0] = format_marker(&ReviewMarker {
        round: 2,
        heads: "primary:000000000000".into(),
        important: Some(0),
        nit: Some(0),
    });
    let out = run(&fake, &input(&e, &repos, &[])).await.expect("new HEAD");
    assert_eq!(out.round, 3);
    let dev = record(&e)["thread_start"][0]["developerInstructions"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        dev.contains(DEFAULT_POLICY.lines().next().unwrap()),
        "built-in policy"
    );
    assert!(fake.pr_comments.lock().unwrap()[0]
        .0
        .contains("审查政策：内置默认"));
}

#[tokio::test]
async fn test_failed_review_reports_unknown_counts_and_clears_label() {
    let e = env(None, true);
    let repos = review_repo(&e);
    let fake = Fake::default();
    let out = run(&fake, &input(&e, &repos, &[])).await.expect("ran");
    assert_eq!(out.important, None);
    let comments = fake.pr_comments.lock().unwrap();
    assert!(comments[0].0.contains("审查未完成"));
    assert!(comments[0].0.contains("Important 未知"));
    let labels = fake.labels.lock().unwrap();
    assert!(labels.contains(&format!("-{LABEL_RUNNING}")));
    assert!(labels.contains(&format!("-{LABEL_IMPORTANT}")));
}

#[tokio::test]
async fn test_reviewer_answers_server_requests_and_skips_junk_lines() {
    let e = env_mode(None, "noise");
    let repos = review_repo(&e);
    let fake = Fake::default();
    let out = run(&fake, &input(&e, &repos, &[])).await.expect("ran");
    assert_eq!(out.important, Some(1), "review still parsed");
    let rec = record(&e);
    assert_eq!(
        rec["reply_78"][0]["result"],
        serde_json::json!({"permissions": {}, "scope": "turn"})
    );
    assert_eq!(rec["reply_79"][0]["result"]["decision"], "denied");
    assert_eq!(rec["reply_80"][0]["error"]["code"], -32601);
}

#[tokio::test]
async fn test_review_errors_are_reported_as_unfinished() {
    for (mode, needle) in [
        ("reject", "review rejected by server"),
        ("exit", "exited"),
        ("hang", "timeout"),
    ] {
        let e = env_mode(None, mode);
        let repos = review_repo(&e);
        let fake = Fake::default();
        let mut inp = input(&e, &repos, &[]);
        inp.timeout = Duration::from_secs(2);
        let out = run(&fake, &inp).await.expect("ran");
        assert_eq!(out.important, None, "{mode}");
        let comments = fake.pr_comments.lock().unwrap();
        assert!(
            comments[0].0.contains("审查未完成"),
            "{mode}: {}",
            comments[0].0
        );
        assert!(
            comments[0].0.to_lowercase().contains(needle),
            "{mode}: {}",
            comments[0].0
        );
    }
}

// ------------------------------------------------------------ blocking

use symphony_platform::agent::ai_review::{block_decision, BlockDecision, ReviewOutcome};
use symphony_platform::config::{ReviewBlockOn, ReviewConfig};

fn cfg(block: bool) -> ReviewConfig {
    ReviewConfig {
        enabled: true,
        block_on: if block {
            ReviewBlockOn::Important
        } else {
            ReviewBlockOn::None
        },
        max_rounds: 2,
        timeout_ms: 1,
    }
}

fn outcome(round: u32, important: Option<usize>) -> ReviewOutcome {
    ReviewOutcome {
        round,
        important,
        nit: Some(0),
    }
}

#[test]
fn test_block_decision_sends_back_until_max_rounds() {
    assert_eq!(
        block_decision(&cfg(true), &outcome(1, Some(2)), false),
        BlockDecision::SendBack
    );
    assert_eq!(
        block_decision(&cfg(true), &outcome(2, Some(1)), false),
        BlockDecision::Unresolved,
        "last round keeps the issue in Human Review"
    );
    assert_eq!(
        block_decision(&cfg(true), &outcome(1, Some(0)), false),
        BlockDecision::Clear
    );
}

#[test]
fn test_block_decision_never_blocks_unknown_blocked_or_disabled() {
    assert_eq!(
        block_decision(&cfg(true), &outcome(1, None), false),
        BlockDecision::Keep,
        "unknown counts never block"
    );
    assert_eq!(
        block_decision(&cfg(true), &outcome(1, Some(3)), true),
        BlockDecision::Keep,
        "blocked hand-offs are not sent back"
    );
    assert_eq!(
        block_decision(&cfg(false), &outcome(1, Some(3)), false),
        BlockDecision::Keep
    );
    assert_eq!(
        block_decision(&cfg(false), &outcome(1, Some(0)), false),
        BlockDecision::Clear
    );
}
