//! Multi-repo workspace contract tests (docs/方案/multi-repo-workspace-design.md §11).
//!
//! A local "platform host" directory with bare repositories stands in for
//! GitLab: `clone_url(slug)` = `<host>/<slug>.git`, so no network is used.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use symphony_platform::config::repos::{CheckoutMode, RepoMode, RepoRole, RepoSpec};
use symphony_platform::config::service_config::HooksConfig;
use symphony_platform::config::service_config::WorkspaceGcConfig;
use symphony_platform::workspace::repo_api::RepoApi;
use symphony_platform::workspace::repos::{RepoCheckoutContext, RepoStatus};
use symphony_platform::workspace::{
    MultiRepoPrepare, WorkspaceError, WorkspaceGc, WorkspaceGcCycleOptions, WorkspaceManager,
};
use tempfile::TempDir;

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Create `<host>/<slug>.git` with one commit on `branch`.
fn make_remote(host: &Path, slug: &str, branch: &str, file: &str) {
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
    std::fs::write(seed.join(file), "hello\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-q", "-m", "init"]);
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

fn specs() -> Vec<RepoSpec> {
    vec![
        RepoSpec {
            alias: "frontend".into(),
            slug: "zimei10525/jst-ai-studio".into(),
            role: RepoRole::Primary,
            default_branch: "dev".into(),
            mode: RepoMode::Write,
            checkout: CheckoutMode::Partial,
            merge_after: vec!["backend".into()],
        },
        RepoSpec {
            alias: "backend".into(),
            slug: "JstAi/jxy".into(),
            role: RepoRole::Secondary,
            default_branch: "pre".into(),
            mode: RepoMode::Read,
            checkout: CheckoutMode::Full,
            merge_after: vec![],
        },
    ]
}

struct Fixture {
    _tmp: TempDir,
    host: PathBuf,
    root: PathBuf,
    ctx: RepoCheckoutContext,
}

fn fixture() -> Fixture {
    let tmp = TempDir::new().unwrap();
    let host = tmp.path().join("host");
    make_remote(&host, "zimei10525/jst-ai-studio", "dev", "app.ts");
    make_remote(&host, "JstAi/jxy", "pre", "Api.cs");
    let root = tmp.path().join("ws");
    std::fs::create_dir_all(&root).unwrap();
    let ctx = RepoCheckoutContext {
        platform_host: format!("file://{}", host.display()),
        token: Some("glpat-test-secret".into()),
        askpass_dir: root.join(".symphony").join("askpass"),
        issue_number: "32".into(),
        clone_timeout: Duration::from_secs(60),
        fetch_timeout: Duration::from_secs(30),
    };
    Fixture {
        _tmp: tmp,
        host,
        root,
        ctx,
    }
}

async fn prepare(
    mgr: &WorkspaceManager,
    f: &Fixture,
    specs: &[RepoSpec],
    run: &str,
) -> Result<
    (
        symphony_platform::workspace::WorkspaceRunLease,
        symphony_platform::workspace::MultiRepoState,
    ),
    WorkspaceError,
> {
    mgr.prepare_multi_repo_workspace(MultiRepoPrepare {
        issue_id: "32",
        issue_identifier: "#32",
        run_id: run,
        service_instance_id: "svc",
        checkout: &f.ctx,
        specs,
        on_progress: &|| {},
    })
    .await
}

#[tokio::test]
async fn first_attempt_checks_out_every_repo_with_guards() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    let (lease, state) = prepare(&mgr, &f, &specs(), "run-1").await.unwrap();
    let ws = lease.workspace.path.clone();

    // Layout: <ws>/<alias>, writable repo on the D4 branch, read-only detached.
    assert!(ws.join("frontend/app.ts").exists());
    assert!(ws.join("backend/Api.cs").exists());
    assert_eq!(
        git(&ws.join("frontend"), &["rev-parse", "--abbrev-ref", "HEAD"]),
        "symphony/jst-ai-studio-32"
    );
    assert_eq!(
        git(&ws.join("backend"), &["rev-parse", "--abbrev-ref", "HEAD"]),
        "HEAD"
    );
    assert_eq!(state.repos["frontend"].status, RepoStatus::Ready);
    assert_eq!(state.repos["backend"].branch, None);

    // Marker is schema 2 and records both repos.
    let marker: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ws.join(".symphony-workspace.json")).unwrap())
            .unwrap();
    assert_eq!(marker["schema_version"], 2);
    assert_eq!(marker["init_status"], "ready");
    assert!(marker["expected_git_root"].is_null());
    assert_eq!(marker["repos"]["backend"]["writable"], false);

    // No token anywhere on disk inside the workspace.
    for repo in ["frontend", "backend"] {
        let cfg = std::fs::read_to_string(ws.join(repo).join(".git/config")).unwrap();
        assert!(
            !cfg.contains("glpat-test-secret"),
            "{repo} config leaks token"
        );
    }
    let askpass = std::fs::read_to_string(f.root.join(".symphony/askpass/git-askpass.sh")).unwrap();
    assert!(!askpass.contains("glpat-test-secret"));
    assert!(!f.root.join(".symphony/askpass").starts_with(&ws));

    // Read-only repo: push URL disabled, files not writable, dirs still writable.
    let push_url = git(
        &ws.join("backend"),
        &["remote", "get-url", "--push", "origin"],
    );
    assert_eq!(push_url, "no-push://disabled");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(ws.join("backend/Api.cs"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o222, 0, "read-only repo file must not be writable");
        let dmode = std::fs::metadata(ws.join("backend"))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(dmode & 0o200, 0, "directories stay writable for GC");
    }
}

#[tokio::test]
async fn readonly_repo_push_is_refused_and_writable_whitelist_enforced() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    let (lease, _) = prepare(&mgr, &f, &specs(), "run-1").await.unwrap();
    let ws = lease.workspace.path.clone();

    // Read-only: even with an explicit URL the pre-push hook refuses.
    let backend = ws.join("backend");
    let real = format!("{}/JstAi/jxy.git", f.host.display());
    let out = Command::new("git")
        .args(["push", &real, "HEAD:refs/heads/evil"])
        .current_dir(&backend)
        .output()
        .unwrap();
    assert!(!out.status.success(), "push from read-only repo must fail");

    // Writable: the work branch passes, a look-alike (#320) and default branch fail.
    let fe = ws.join("frontend");
    std::fs::write(fe.join("app.ts"), "changed\n").unwrap();
    git(
        &fe,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@e",
            "commit",
            "-q",
            "-am",
            "work",
        ],
    );
    let push = |refspec: &str| {
        Command::new("git")
            .args(["push", "origin", refspec])
            .current_dir(&fe)
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(push("HEAD:refs/heads/symphony/jst-ai-studio-32"));
    assert!(!push("HEAD:refs/heads/symphony/jst-ai-studio-320"));
    assert!(!push("HEAD:refs/heads/dev"));
}

#[tokio::test]
async fn retry_only_fetches_and_keeps_unpushed_commits() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    let (lease, _) = prepare(&mgr, &f, &specs(), "run-1").await.unwrap();
    let fe = lease.workspace.path.join("frontend");
    std::fs::write(fe.join("local.txt"), "wip\n").unwrap();
    git(&fe, &["add", "local.txt"]);
    git(
        &fe,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@e",
            "commit",
            "-q",
            "-m",
            "unpushed",
        ],
    );
    let head = git(&fe, &["rev-parse", "HEAD"]);
    drop(lease);

    // Remote branch for the work branch does not exist yet: refresh must not fail.
    let (lease2, state) = prepare(&mgr, &f, &specs(), "run-2").await.unwrap();
    assert_eq!(
        git(
            &lease2.workspace.path.join("frontend"),
            &["rev-parse", "HEAD"]
        ),
        head
    );
    assert!(!lease2.workspace.created_now);
    assert_eq!(state.repos["frontend"].last_error, None);
}

#[tokio::test]
async fn fetch_failure_keeps_repo_ready_and_attempt_continues() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    let (lease, _) = prepare(&mgr, &f, &specs(), "run-1").await.unwrap();
    drop(lease);
    // Break the remote of the read-only repo.
    std::fs::rename(f.host.join("JstAi/jxy.git"), f.host.join("JstAi/gone.git")).unwrap();
    let (_, state) = prepare(&mgr, &f, &specs(), "run-2").await.unwrap();
    assert_eq!(state.repos["backend"].status, RepoStatus::Ready);
    assert!(state.repos["backend"].last_error.is_some());
}

#[tokio::test]
async fn interrupted_initialization_is_reentered_automatically() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    // First attempt fails on the second repo (remote missing).
    std::fs::rename(f.host.join("JstAi/jxy.git"), f.host.join("JstAi/gone.git")).unwrap();
    let err = prepare(&mgr, &f, &specs(), "run-1").await.unwrap_err();
    assert!(matches!(err, WorkspaceError::RepoCheckout { .. }), "{err}");
    // Simulate a leftover half clone in tmp.
    let ws = f.root.join("i-3332-_32");
    std::fs::create_dir_all(ws.join(".symphony/tmp/backend-123/.git")).unwrap();
    let marker: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ws.join(".symphony-workspace.json")).unwrap())
            .unwrap();
    assert_eq!(marker["init_status"], "initializing");

    // Remote comes back: the next attempt re-enters without manual recovery.
    std::fs::rename(f.host.join("JstAi/gone.git"), f.host.join("JstAi/jxy.git")).unwrap();
    let (lease, state) = prepare(&mgr, &f, &specs(), "run-2").await.unwrap();
    assert!(lease.workspace.path.join("backend/Api.cs").exists());
    assert_eq!(state.repos["backend"].status, RepoStatus::Ready);
    assert!(
        !ws.join(".symphony/tmp").exists(),
        "tmp leftovers are cleared"
    );
}

#[tokio::test]
async fn renamed_repo_without_ready_marker_is_adopted() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    let (lease, _) = prepare(&mgr, &f, &specs(), "run-1").await.unwrap();
    let meta_path = lease.workspace.path.join(".symphony-workspace.json");
    drop(lease);
    // Crash after rename, before the marker was written: drop backend state.
    let mut marker: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&meta_path).unwrap()).unwrap();
    marker["repos"].as_object_mut().unwrap().remove("backend");
    marker["init_status"] = "initializing".into();
    std::fs::write(&meta_path, serde_json::to_vec(&marker).unwrap()).unwrap();

    let (_, state) = prepare(&mgr, &f, &specs(), "run-2").await.unwrap();
    assert_eq!(state.repos["backend"].status, RepoStatus::Ready);
}

#[tokio::test]
async fn unrelated_directory_at_alias_requires_manual_recovery() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    let ws = f.root.join("i-3332-_32");
    std::fs::create_dir_all(ws.join("backend")).unwrap();
    // An unknown non-empty workspace is refused like in single-repo mode.
    let err = prepare(&mgr, &f, &specs(), "run-1").await.unwrap_err();
    assert!(
        matches!(err, WorkspaceError::ManualRecoveryRequired { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn v1_workspace_is_not_migrated() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    let lease = mgr
        .prepare_issue_workspace("32", "#32", "run-0", "svc")
        .await
        .unwrap();
    drop(lease);
    let err = prepare(&mgr, &f, &specs(), "run-1").await.unwrap_err();
    match err {
        WorkspaceError::ManualRecoveryRequired { reason } => {
            assert_eq!(reason, "workspace_v1_not_migrated")
        }
        other => panic!("unexpected {other}"),
    }
    // The runner detects this up front and keeps the single-repo layout.
    assert!(mgr.has_legacy_workspace("32", "#32").await);
    mgr.prepare_issue_workspace("32", "#32", "run-2", "svc")
        .await
        .unwrap();
}

#[tokio::test]
async fn readonly_violation_is_detected_after_run() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    let (lease, _) = prepare(&mgr, &f, &specs(), "run-1").await.unwrap();
    assert!(mgr
        .readonly_violations(&lease, &f.ctx, &specs())
        .await
        .is_empty());
    let be = lease.workspace.path.join("backend");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(be.join("Api.cs"), std::fs::Permissions::from_mode(0o644))
            .unwrap();
    }
    std::fs::write(be.join("Api.cs"), "tampered\n").unwrap();
    let v = mgr.readonly_violations(&lease, &f.ctx, &specs()).await;
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].contains("backend"));
}

#[derive(Default)]
struct FakeApi {
    open: Mutex<BTreeMap<(String, String), Vec<u64>>>,
    closed: Mutex<Vec<(String, u64)>>,
    fail_close: Mutex<bool>,
}

#[async_trait]
impl RepoApi for FakeApi {
    async fn latest_label_add_event(
        &self,
        _iid: &str,
        _label: &str,
    ) -> Result<Option<u64>, String> {
        Ok(None)
    }
    async fn open_mrs_for_branch(&self, slug: &str, branch: &str) -> Result<Vec<u64>, String> {
        Ok(self
            .open
            .lock()
            .unwrap()
            .get(&(slug.to_string(), branch.to_string()))
            .cloned()
            .unwrap_or_default())
    }
    async fn close_mr(&self, slug: &str, iid: u64) -> Result<(), String> {
        if *self.fail_close.lock().unwrap() {
            return Err("boom".into());
        }
        self.closed.lock().unwrap().push((slug.to_string(), iid));
        for v in self.open.lock().unwrap().values_mut() {
            v.retain(|x| *x != iid);
        }
        Ok(())
    }
}

#[tokio::test]
async fn rework_switches_once_per_label_event_and_is_idempotent() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    let (lease, _) = prepare(&mgr, &f, &specs(), "run-1").await.unwrap();
    let fe = lease.workspace.path.join("frontend");
    let api = Arc::new(FakeApi::default());
    api.open.lock().unwrap().insert(
        (
            "zimei10525/jst-ai-studio".into(),
            "symphony/jst-ai-studio-32".into(),
        ),
        vec![87],
    );

    // First Rework (event 7001): close !87, switch to -r2.
    let s = mgr
        .apply_rework(&lease, &f.ctx, &specs(), 7001, api.as_ref())
        .await
        .unwrap();
    assert_eq!(
        s.repos["frontend"].branch.as_deref(),
        Some("symphony/jst-ai-studio-32-r2")
    );
    assert_eq!(
        git(&fe, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "symphony/jst-ai-studio-32-r2"
    );
    assert_eq!(
        api.closed.lock().unwrap().as_slice(),
        &[("zimei10525/jst-ai-studio".to_string(), 87)]
    );
    assert_eq!(
        s.repos["backend"].branch, None,
        "read-only repo never switches"
    );

    // Retry with the same event: no-op.
    let s = mgr
        .apply_rework(&lease, &f.ctx, &specs(), 7001, api.as_ref())
        .await
        .unwrap();
    assert_eq!(
        s.repos["frontend"].branch.as_deref(),
        Some("symphony/jst-ai-studio-32-r2")
    );

    // Second Rework (new event id): -r3.
    let s = mgr
        .apply_rework(&lease, &f.ctx, &specs(), 7100, api.as_ref())
        .await
        .unwrap();
    assert_eq!(
        s.repos["frontend"].branch.as_deref(),
        Some("symphony/jst-ai-studio-32-r3")
    );
}

#[tokio::test]
async fn rework_failure_mid_way_does_not_double_increment() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    let (lease, _) = prepare(&mgr, &f, &specs(), "run-1").await.unwrap();
    let api = Arc::new(FakeApi::default());
    api.open.lock().unwrap().insert(
        (
            "zimei10525/jst-ai-studio".into(),
            "symphony/jst-ai-studio-32".into(),
        ),
        vec![87],
    );
    *api.fail_close.lock().unwrap() = true;
    assert!(mgr
        .apply_rework(&lease, &f.ctx, &specs(), 7001, api.as_ref())
        .await
        .is_err());
    *api.fail_close.lock().unwrap() = false;
    let s = mgr
        .apply_rework(&lease, &f.ctx, &specs(), 7001, api.as_ref())
        .await
        .unwrap();
    assert_eq!(
        s.repos["frontend"].branch.as_deref(),
        Some("symphony/jst-ai-studio-32-r2")
    );
}

#[tokio::test]
async fn rework_after_gc_rebuild_does_not_reuse_remote_branch_number() {
    let f = fixture();
    // Remote already has -r2 from a previous (GC'd) workspace.
    let seed = f.host.join("seed-zimei10525-jst-ai-studio");
    git(
        &seed,
        &[
            "push",
            "-q",
            f.host
                .join("zimei10525/jst-ai-studio.git")
                .to_str()
                .unwrap(),
            "HEAD:refs/heads/symphony/jst-ai-studio-32-r2",
        ],
    );
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    let (lease, _) = prepare(&mgr, &f, &specs(), "run-1").await.unwrap();
    let api = Arc::new(FakeApi::default());
    let s = mgr
        .apply_rework(&lease, &f.ctx, &specs(), 9000, api.as_ref())
        .await
        .unwrap();
    assert_eq!(
        s.repos["frontend"].branch.as_deref(),
        Some("symphony/jst-ai-studio-32-r3")
    );
}

#[tokio::test]
async fn gc_deletes_workspace_with_readonly_files() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    let (lease, _) = prepare(&mgr, &f, &specs(), "run-1").await.unwrap();
    let ws = lease.workspace.path.clone();
    drop(lease);
    mgr.write_terminal_marker_at(
        "32",
        "#32",
        "Done",
        chrono::Utc::now() - chrono::Duration::hours(2),
    )
    .await
    .unwrap();
    let gc = WorkspaceGc::new(
        mgr,
        WorkspaceGcConfig {
            gc_interval_ms: 0,
            gc_retention_ms: 60_000,
            gc_batch_size: 10,
            gc_cycle_timeout_ms: 120_000,
        },
    );
    let summary = gc
        .run_cycle(WorkspaceGcCycleOptions::default())
        .await
        .unwrap();
    assert_eq!(summary.deleted, 1);
    assert!(!ws.exists());
}

#[tokio::test]
async fn after_create_runs_after_checkout_and_failure_quarantines() {
    let f = fixture();
    let hooks = HooksConfig {
        after_create: Some(
            "test -f frontend/app.ts && test -f backend/Api.cs && touch hook-ran".into(),
        ),
        ..HooksConfig::default()
    };
    let mgr = WorkspaceManager::new(f.root.clone(), hooks);
    let (lease, _) = prepare(&mgr, &f, &specs(), "run-1").await.unwrap();
    assert!(lease.workspace.path.join("hook-ran").exists());
    drop(lease);

    let failing = WorkspaceManager::new(
        f.root.join("other"),
        HooksConfig {
            after_create: Some("exit 3".into()),
            ..HooksConfig::default()
        },
    );
    std::fs::create_dir_all(f.root.join("other")).unwrap();
    let err = prepare(&failing, &f, &specs(), "run-1").await.unwrap_err();
    assert!(matches!(err, WorkspaceError::HookFailed { .. }), "{err}");
    let q = f.root.join("other/.symphony/quarantine/i-3332");
    assert!(
        q.join("run-1/backend/Api.cs").exists(),
        "workspace moved to quarantine"
    );
}

#[tokio::test]
async fn rework_with_uncommitted_changes_stashes_instead_of_failing() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    let (lease, _) = prepare(&mgr, &f, &specs(), "run-1").await.unwrap();
    let fe = lease.workspace.path.join("frontend");
    std::fs::write(fe.join("app.ts"), "dirty edit\n").unwrap();
    std::fs::write(fe.join("new-file.ts"), "untracked\n").unwrap();
    let api = Arc::new(FakeApi::default());
    let s = mgr
        .apply_rework(&lease, &f.ctx, &specs(), 7001, api.as_ref())
        .await
        .unwrap();
    assert_eq!(
        s.repos["frontend"].branch.as_deref(),
        Some("symphony/jst-ai-studio-32-r2")
    );
    assert_eq!(
        std::fs::read_to_string(fe.join("app.ts")).unwrap(),
        "hello\n"
    );
    assert!(!fe.join("new-file.ts").exists());
    let stash = git(&fe, &["stash", "list"]);
    assert!(
        stash.contains("symphony: WIP before symphony/jst-ai-studio-32-r2"),
        "{stash}"
    );
}

#[tokio::test]
async fn new_or_multi_repo_workspace_is_not_legacy() {
    let f = fixture();
    let mgr = WorkspaceManager::new(f.root.clone(), HooksConfig::default());
    assert!(!mgr.has_legacy_workspace("32", "#32").await);
    let (lease, _) = prepare(&mgr, &f, &specs(), "run-1").await.unwrap();
    drop(lease);
    assert!(!mgr.has_legacy_workspace("32", "#32").await);
}
