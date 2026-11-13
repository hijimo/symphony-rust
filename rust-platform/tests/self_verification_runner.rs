//! Runner-level contract tests for self-verification (docs/方案/self-verification-and-agents-md.md).
//!
//! A Python fake app-server records what Symphony sends (developer
//! instructions, first-turn prompt, environment) and replays a sub-agent
//! `turn/completed` before the main thread's, plus a v2 approval request.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

use symphony_platform::agent::{AgentIssue, AgentRunner, IssueStateRefresher};
use symphony_platform::config::repos::{CheckoutMode, RepoMode, RepoRole, RepoSpec};
use symphony_platform::config::service_config::{CodexConfig, HooksConfig, ServiceConfig};
use symphony_platform::config::{ConfigHolder, EffectiveConfig};
use symphony_platform::prompt::PromptEngine;
use symphony_platform::workspace::WorkspaceManager;

const FAKE_SERVER: &str = r#"
import json, os, sys
rec = os.environ["FAKE_RECORD"]
def out(o):
    sys.stdout.write(json.dumps(o) + "\n"); sys.stdout.flush()
def save(k, v):
    try: d = json.load(open(rec))
    except Exception: d = {}
    d[k] = v; json.dump(d, open(rec, "w"))
save("env", {k: os.environ.get(k) for k in ["SYMPHONY_RUN_ID", "SYMPHONY_E2E_ENV_FILE"]})
save("cwd", os.getcwd())
for line in sys.stdin:
    m = json.loads(line)
    meth = m.get("method")
    if meth == "initialize":
        out({"jsonrpc": "2.0", "id": m["id"], "result": {}})
    elif meth == "thread/start":
        save("dev", m["params"].get("developerInstructions"))
        out({"jsonrpc": "2.0", "id": m["id"], "result": {"thread": {"id": "main-t"}}})
    elif meth == "turn/start":
        save("prompt", m["params"]["input"][0]["text"])
        save("thread", m["params"].get("threadId"))
        out({"jsonrpc": "2.0", "id": m["id"], "result": {"turn": {"id": "turn-1"}}})
        out({"method": "turn/started", "params": {"threadId": "sub-t", "turn": {"id": "s1"}}})
        out({"jsonrpc": "2.0", "id": 900, "method": "item/commandExecution/requestApproval",
             "params": {"threadId": "sub-t", "turnId": "s1", "itemId": "i", "startedAtMs": 1}})
        # Sub-agent finishes first: must NOT end the main turn.
        out({"method": "turn/completed", "params": {"threadId": "sub-t", "turn": {"id": "s1", "status": "completed"}}})
    elif m.get("id") == 900:
        save("approval_reply", m)
        hook = os.environ.get("FAKE_BEFORE_DONE")
        if hook and os.path.exists(hook): os.system("sh " + hook)
        out({"method": "turn/completed", "params": {"threadId": "main-t", "turn": {"id": "turn-1", "status": "completed"}}})
"#;

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn make_remote(host: &Path, slug: &str, branch: &str, agents: Option<&str>) {
    let bare = host.join(format!("{slug}.git"));
    std::fs::create_dir_all(&bare).unwrap();
    git(
        &bare,
        &[
            "init",
            "--bare",
            "-q",
            &format!("--initial-branch={branch}"),
        ],
    );
    let seed = host.join(format!("seed-{}", slug.replace('/', "-")));
    std::fs::create_dir_all(&seed).unwrap();
    git(
        &seed,
        &["init", "-q", &format!("--initial-branch={branch}")],
    );
    if let Some(a) = agents {
        std::fs::write(seed.join("AGENTS.md"), a).unwrap();
    }
    std::fs::write(seed.join("package.json"), "{}").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-qm", "init"]);
    git(
        &seed,
        &[
            "push",
            "-q",
            bare.to_str().unwrap(),
            &format!("HEAD:refs/heads/{branch}"),
        ],
    );
}

/// Workpad / label fake.
#[derive(Default)]
struct Fake {
    state_after: Mutex<String>,
    workpad: Mutex<String>,
    labels_added: Mutex<Vec<String>>,
    labels_removed: Mutex<Vec<String>>,
}

#[async_trait]
impl IssueStateRefresher for Fake {
    async fn refresh_issue_state(&self, id: &str) -> Result<Option<AgentIssue>, String> {
        Ok(Some(issue(id, &self.state_after.lock().unwrap(), None)))
    }
    fn is_terminal_state(&self, _s: &str) -> bool {
        false
    }
    fn is_active_state(&self, s: &str) -> bool {
        matches!(s, "Todo" | "In Progress" | "Rework" | "Merging")
    }
    fn supports_workpad(&self) -> bool {
        true
    }
    async fn fetch_workpad(&self, _id: &str) -> Result<Option<(u64, String)>, String> {
        Ok(Some((1, self.workpad.lock().unwrap().clone())))
    }
    async fn update_workpad(&self, _c: u64, body: &str) -> Result<(), String> {
        *self.workpad.lock().unwrap() = body.to_string();
        Ok(())
    }
    async fn add_labels(&self, _id: &str, labels: &[String]) -> Result<(), String> {
        self.labels_added
            .lock()
            .unwrap()
            .extend(labels.iter().cloned());
        Ok(())
    }
    async fn remove_labels(&self, _id: &str, labels: &[String]) -> Result<(), String> {
        self.labels_removed
            .lock()
            .unwrap()
            .extend(labels.iter().cloned());
        Ok(())
    }
}

fn issue(id: &str, state: &str, description: Option<&str>) -> AgentIssue {
    AgentIssue {
        id: id.to_string(),
        identifier: format!("#{id}"),
        title: "T".into(),
        description: description.map(str::to_string),
        priority: None,
        state: state.into(),
        labels: vec![],
        url: None,
        branch_name: None,
        blocked_by: vec![],
        created_at: None,
        updated_at: None,
    }
}

struct Env {
    _tmp: TempDir,
    root: PathBuf,
    host: PathBuf,
    record: PathBuf,
    server: PathBuf,
}

fn env() -> Env {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("ws");
    std::fs::create_dir_all(&root).unwrap();
    let host = tmp.path().join("host");
    let server = tmp.path().join("fake_server.py");
    std::fs::write(&server, FAKE_SERVER).unwrap();
    Env {
        record: tmp.path().join("record.json"),
        root,
        host,
        server,
        _tmp: tmp,
    }
}

const DEFAULT_TEMPLATE: &str =
    "PROMPT {{ issue.identifier }} run={{ run_id }} layout={{ workspace_layout }}";

fn runner(e: &Env, service: ServiceConfig, before_done: &str) -> AgentRunner {
    runner_with_template(e, service, before_done, DEFAULT_TEMPLATE)
}

fn runner_with_template(
    e: &Env,
    service: ServiceConfig,
    before_done: &str,
    template: &str,
) -> AgentRunner {
    let hook = e.root.join(format!("hook-{}.sh", std::process::id()));
    let hook = hook.with_extension(format!("{}.sh", before_done.len()));
    std::fs::write(&hook, before_done).unwrap();
    let before_done = hook.display().to_string();
    let service = ServiceConfig {
        workspace_root: e.root.clone(),
        max_turns: 1,
        codex: CodexConfig {
            command: format!(
                "FAKE_RECORD='{}' FAKE_BEFORE_DONE='{}' python3 '{}'",
                e.record.display(),
                before_done,
                e.server.display()
            ),
            read_timeout_ms: 5_000,
            turn_timeout_ms: 20_000,
            ..CodexConfig::default()
        },
        ..service
    };
    let holder = Arc::new(ConfigHolder::new(
        EffectiveConfig {
            service,
            prompt_template: template.to_string(),
            loaded_at: Utc::now(),
        },
        e.root.join("WORKFLOW.md"),
    ));
    let engine = Arc::new(PromptEngine::compile(template).unwrap());
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    AgentRunner::new(
        Arc::new(WorkspaceManager::new(
            e.root.clone(),
            HooksConfig::default(),
        )),
        holder,
        engine,
        tx,
        CancellationToken::new(),
    )
}

fn record(e: &Env) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(&e.record).unwrap()).unwrap()
}

fn set_gitlab_env(host: &Path) {
    // checkout_context() reads GITLAB_HOST; a file:// host stands in for GitLab.
    std::env::set_var("GITLAB_HOST", format!("file://{}", host.display()));
    std::env::remove_var("GITLAB_TOKEN");
}

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn multi_repo_run_injects_agents_md_ignores_subagent_completion_and_labels_blocked() {
    let _g = ENV_LOCK.lock().await;
    let e = env();
    set_gitlab_env(&e.host);
    make_remote(&e.host, "g/fe", "dev", Some("FE-ORIGIN-RULES\n"));
    make_remote(&e.host, "g/ro", "main", Some("RO-RULES-NOT-INJECTED\n"));
    let specs = vec![
        RepoSpec {
            alias: "fe".into(),
            slug: "g/fe".into(),
            role: RepoRole::Primary,
            mode: RepoMode::Write,
            default_branch: "dev".into(),
            checkout: CheckoutMode::Partial,
            merge_after: vec![],
        },
        RepoSpec {
            alias: "ro".into(),
            slug: "g/ro".into(),
            role: RepoRole::Secondary,
            mode: RepoMode::Read,
            default_branch: "main".into(),
            checkout: CheckoutMode::Partial,
            merge_after: vec![],
        },
    ];
    // The agent writes a blocked marker for *this* run (run id from env).
    let marker_cmd = "true";
    let r = runner(
        &e,
        ServiceConfig {
            workspace_repos: specs,
            ..ServiceConfig::default()
        },
        marker_cmd,
    );
    let fake = Arc::new(Fake::default());
    *fake.state_after.lock().unwrap() = "Human Review".into();
    *fake.workpad.lock().unwrap() =
        "## Codex Workpad\n<!-- symphony:blocked run=old-run reason=validation heads=fe:aaaaaaaaaaaa -->\n".into();
    let desc = "## E2E\n- 环境变量: E2E_TOKEN=tok-123\n";
    r.run_attempt(issue("41", "In Progress", Some(desc)), None, fake.clone())
        .await
        .unwrap();

    let rec = record(&e);
    let prompt = rec["prompt"].as_str().unwrap();
    assert!(
        prompt.starts_with("<<<SYMPHONY-REPO-INSTRUCTIONS nonce="),
        "{prompt}"
    );
    assert!(prompt.contains("FE-ORIGIN-RULES"));
    assert!(!prompt.contains("RO-RULES-NOT-INJECTED"));
    assert!(prompt.contains("layout=multi_repo"));
    assert!(
        !prompt.contains("tok-123"),
        "token must not reach the prompt"
    );
    let run_id = rec["env"]["SYMPHONY_RUN_ID"].as_str().unwrap().to_string();
    assert!(prompt.contains(&format!("run={run_id}")));
    assert!(rec["dev"]
        .as_str()
        .unwrap()
        .contains("Symphony safety rules"));
    assert_eq!(rec["thread"], "main-t");
    // v2 approval answered with a JSON-RPC result carrying the request id.
    assert_eq!(rec["approval_reply"]["id"], 900);
    assert_eq!(rec["approval_reply"]["result"]["decision"], "accept");
    // E2E env file existed for the run and was removed afterwards.
    let env_file = rec["env"]["SYMPHONY_E2E_ENV_FILE"].as_str().unwrap();
    assert!(env_file.ends_with("e2e.env"));
    assert!(
        !Path::new(env_file).exists(),
        "E2E file removed after the run"
    );

    // Run start removed labels and cleared the previous run's marker; that
    // stale marker did not produce a label at the end.
    assert!(fake
        .labels_removed
        .lock()
        .unwrap()
        .contains(&"validation-blocked".to_string()));
    assert!(fake
        .workpad
        .lock()
        .unwrap()
        .contains("symphony:blocked-cleared run=old-run"));
    assert!(fake.labels_added.lock().unwrap().is_empty());
}

#[tokio::test]
async fn effective_marker_of_this_run_adds_label_even_from_merging() {
    let _g = ENV_LOCK.lock().await;
    let e = env();
    set_gitlab_env(&e.host);
    make_remote(&e.host, "g/app", "dev", Some("APP\n"));
    // Before finishing, the fake agent appends a marker with its own run id
    // and current head to a file the refresher mirrors into the workpad.
    let mirror = e.root.join("marker.txt");
    let cmd = format!(
        "printf '<!-- symphony:blocked run=%s reason=validation heads=primary:%s -->' \"$SYMPHONY_RUN_ID\" \"$(git rev-parse --short=12 HEAD)\" > {}",
        mirror.display()
    );
    let r = runner(
        &e,
        ServiceConfig {
            workspace_single_repo: Some(
                symphony_platform::config::service_config::SingleRepoSpec {
                    slug: "g/app".into(),
                    default_branch: "dev".into(),
                },
            ),
            ..ServiceConfig::default()
        },
        &cmd,
    );
    struct Mirror {
        inner: Fake,
        file: PathBuf,
    }
    #[async_trait]
    impl IssueStateRefresher for Mirror {
        async fn refresh_issue_state(&self, id: &str) -> Result<Option<AgentIssue>, String> {
            self.inner.refresh_issue_state(id).await
        }
        fn is_terminal_state(&self, s: &str) -> bool {
            self.inner.is_terminal_state(s)
        }
        fn is_active_state(&self, s: &str) -> bool {
            self.inner.is_active_state(s)
        }
        fn supports_workpad(&self) -> bool {
            true
        }
        async fn fetch_workpad(&self, _id: &str) -> Result<Option<(u64, String)>, String> {
            let m = std::fs::read_to_string(&self.file).unwrap_or_default();
            Ok(Some((1, format!("## Codex Workpad\n{m}\n"))))
        }
        async fn add_labels(&self, id: &str, l: &[String]) -> Result<(), String> {
            self.inner.add_labels(id, l).await
        }
        async fn remove_labels(&self, id: &str, l: &[String]) -> Result<(), String> {
            self.inner.remove_labels(id, l).await
        }
    }
    let fake = Arc::new(Mirror {
        inner: Fake::default(),
        file: mirror.clone(),
    });
    *fake.inner.state_after.lock().unwrap() = "Human Review".into();
    r.run_attempt(issue("42", "Merging", None), None, fake.clone())
        .await
        .unwrap();
    let rec = record(&e);
    // single_root: repository at the root, no injected block (Codex loads AGENTS.md).
    let prompt = rec["prompt"].as_str().unwrap();
    assert!(!prompt.contains("SYMPHONY-REPO-INSTRUCTIONS"));
    assert!(prompt.contains("layout=single_root"));
    let cwd = PathBuf::from(rec["cwd"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(cwd.join("AGENTS.md")).unwrap(),
        "APP\n"
    );
    // Merging run does not clear labels, but recognizes its own marker.
    assert!(fake.inner.labels_removed.lock().unwrap().is_empty());
    assert_eq!(
        *fake.inner.labels_added.lock().unwrap(),
        vec!["validation-blocked".to_string()]
    );

    // Next run (human removed the label and moved to Merging again): the old
    // marker belongs to a previous run, so the label is not re-added.
    let r2 = runner(
        &e,
        ServiceConfig {
            workspace_single_repo: Some(
                symphony_platform::config::service_config::SingleRepoSpec {
                    slug: "g/app".into(),
                    default_branch: "dev".into(),
                },
            ),
            ..ServiceConfig::default()
        },
        "true",
    );
    fake.inner.labels_added.lock().unwrap().clear();
    r2.run_attempt(issue("42", "Merging", None), None, fake.clone())
        .await
        .unwrap();
    assert!(fake.inner.labels_added.lock().unwrap().is_empty());
    let _ = BTreeMap::<u8, u8>::new();
    let _ = Duration::from_secs(0);
}

/// Refresher that serves a change-docs listing and records the request.
#[derive(Default)]
struct DocsRefresher {
    requests: Mutex<Vec<(String, String, String)>>,
}

#[async_trait]
impl IssueStateRefresher for DocsRefresher {
    async fn refresh_issue_state(&self, id: &str) -> Result<Option<AgentIssue>, String> {
        Ok(Some(issue(id, "Human Review", None)))
    }
    fn is_terminal_state(&self, _s: &str) -> bool {
        false
    }
    fn is_active_state(&self, s: &str) -> bool {
        matches!(s, "Todo" | "In Progress")
    }
    async fn list_repo_directory(
        &self,
        slug: &str,
        path: &str,
        git_ref: &str,
    ) -> Result<Option<Vec<symphony_platform::platform::RepoFile>>, String> {
        self.requests
            .lock()
            .unwrap()
            .push((slug.into(), path.into(), git_ref.into()));
        Ok(Some(vec![symphony_platform::platform::RepoFile {
            name: "plan.md".into(),
            path: format!("{path}/plan.md"),
            blob_sha: "abcdef0123456789".into(),
        }]))
    }
}

#[tokio::test]
async fn first_turn_prompt_lists_approved_change_docs() {
    let _g = ENV_LOCK.lock().await;
    let e = env();
    let r = runner_with_template(
        &e,
        ServiceConfig {
            tracker_project_slug: "acme/app".into(),
            ..ServiceConfig::default()
        },
        "true",
        "{% if change_docs %}DOCS {% for f in change_docs.files %}{{ change_docs.dir }}/{{ f.name }}@{{ f.blob_sha }}{% endfor %}{% endif %} [{{ change_docs_status }}]",
    );
    let refresher = Arc::new(DocsRefresher::default());
    r.run_attempt(issue("57", "Todo", None), None, refresher.clone())
        .await
        .unwrap();

    let prompt = record(&e)["prompt"].as_str().unwrap().to_string();
    assert!(
        prompt.contains("DOCS docs/changes/57/plan.md@abcdef012345 [present]"),
        "{prompt}"
    );
    assert_eq!(
        refresher.requests.lock().unwrap().as_slice(),
        &[(
            "acme/app".to_string(),
            "docs/changes/57".to_string(),
            String::new()
        )],
        "one lookup, default branch"
    );
}

/// Records plan-check side effects of a run that ends in Human Review.
#[derive(Default)]
struct PlanRefresher {
    workpad: Mutex<String>,
    added: Mutex<Vec<String>>,
    pr_comments: Mutex<Vec<String>>,
}

#[async_trait]
impl IssueStateRefresher for PlanRefresher {
    async fn refresh_issue_state(&self, id: &str) -> Result<Option<AgentIssue>, String> {
        Ok(Some(issue(id, "Human Review", None)))
    }
    fn is_terminal_state(&self, _s: &str) -> bool {
        false
    }
    fn is_active_state(&self, s: &str) -> bool {
        matches!(s, "Todo" | "In Progress")
    }
    fn supports_workpad(&self) -> bool {
        true
    }
    async fn fetch_workpad(&self, _id: &str) -> Result<Option<(u64, String)>, String> {
        Ok(Some((1, self.workpad.lock().unwrap().clone())))
    }
    async fn update_workpad(&self, _c: u64, body: &str) -> Result<(), String> {
        *self.workpad.lock().unwrap() = body.to_string();
        Ok(())
    }
    async fn add_labels(&self, _id: &str, l: &[String]) -> Result<(), String> {
        self.added.lock().unwrap().extend(l.iter().cloned());
        Ok(())
    }
    async fn remove_labels(&self, _id: &str, _l: &[String]) -> Result<(), String> {
        Ok(())
    }
    async fn find_open_pull_request(
        &self,
        slug: &str,
        _branch: &str,
    ) -> Result<Option<symphony_platform::platform::PullRequestRef>, String> {
        Ok(Some(symphony_platform::platform::PullRequestRef {
            repo_slug: slug.into(),
            number: 3,
            url: "u".into(),
        }))
    }
    async fn upsert_pull_request_comment(
        &self,
        _pr: &symphony_platform::platform::PullRequestRef,
        _marker: &str,
        body: &str,
        _mode: symphony_platform::platform::CommentMode,
    ) -> Result<(), String> {
        self.pr_comments.lock().unwrap().push(body.into());
        Ok(())
    }
}

#[tokio::test]
async fn development_run_handing_off_runs_plan_deviation_check() {
    let _g = ENV_LOCK.lock().await;
    let e = env();
    // Remote main already carries the approved plan.
    let bare = e.host.join("acme/app.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare", "-q", "--initial-branch=main"]);
    let seed = e.host.join("seed-app");
    std::fs::create_dir_all(seed.join("docs/changes/61")).unwrap();
    git(&seed, &["init", "-q", "--initial-branch=main"]);
    std::fs::write(
        seed.join("docs/changes/61/plan.md"),
        "## Files\n- src/planned.rs\n",
    )
    .unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-qm", "docs"]);
    git(
        &seed,
        &["push", "-q", bare.to_str().unwrap(), "HEAD:refs/heads/main"],
    );

    // The fake agent clones into the workspace root and commits an unplanned file.
    let agent = format!(
        "set -e\ngit init -q -b main .\ngit remote add origin '{}'\ngit fetch -q origin\n\
         git checkout -q -b feat/61 origin/main\ngit remote set-head origin main\n\
         mkdir -p src && echo 1 > src/other.rs\ngit add src\n\
         git -c user.name=t -c user.email=t@e commit -qm work\n",
        bare.display()
    );
    let r = runner(
        &e,
        ServiceConfig {
            tracker_project_slug: "acme/app".into(),
            ..ServiceConfig::default()
        },
        &agent,
    );
    let refresher = Arc::new(PlanRefresher::default());
    *refresher.workpad.lock().unwrap() = "## Codex Workpad\n".into();
    r.run_attempt(issue("61", "In Progress", None), None, refresher.clone())
        .await
        .unwrap();

    assert!(
        refresher
            .added
            .lock()
            .unwrap()
            .contains(&"plan-deviation".to_string()),
        "labels: {:?}",
        refresher.added.lock().unwrap()
    );
    let comments = refresher.pr_comments.lock().unwrap();
    assert_eq!(comments.len(), 1);
    assert!(comments[0].contains("`src/other.rs`"), "{}", comments[0]);
    assert!(comments[0].contains("`src/planned.rs`"));
    assert!(refresher
        .workpad
        .lock()
        .unwrap()
        .contains("### Symphony plan check"));
}
