//! single_root workspace contract tests (docs/方案/self-verification-and-agents-md.md §4.2).
//!
//! A local directory with a bare repository stands in for GitLab.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use symphony_platform::config::service_config::{HooksConfig, SingleRepoSpec};
use symphony_platform::workspace::repos::RepoCheckoutContext;
use symphony_platform::workspace::{
    SingleRootPrepare, WorkspaceError, WorkspaceLayout, WorkspaceManager,
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
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct Fx {
    _tmp: TempDir,
    host: PathBuf,
    root: PathBuf,
    ctx: RepoCheckoutContext,
    spec: SingleRepoSpec,
}

fn fixture() -> Fx {
    let tmp = TempDir::new().unwrap();
    let host = tmp.path().join("host");
    let slug = "g/app";
    let bare = host.join(format!("{slug}.git"));
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare", "-q", "--initial-branch=dev"]);
    let seed = tmp.path().join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "-q", "--initial-branch=dev"]);
    std::fs::write(seed.join("AGENTS.md"), "APP RULES\n").unwrap();
    std::fs::write(seed.join("package.json"), "{}\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-qm", "init"]);
    git(
        &seed,
        &["push", "-q", bare.to_str().unwrap(), "HEAD:refs/heads/dev"],
    );
    let root = tmp.path().join("ws");
    std::fs::create_dir_all(&root).unwrap();
    let ctx = RepoCheckoutContext {
        platform_host: format!("file://{}", host.display()),
        token: None,
        askpass_dir: root.join(".symphony").join("askpass"),
        issue_number: "8".into(),
        clone_timeout: Duration::from_secs(60),
        fetch_timeout: Duration::from_secs(30),
    };
    Fx {
        _tmp: tmp,
        host,
        root,
        ctx,
        spec: SingleRepoSpec {
            slug: slug.into(),
            default_branch: "dev".into(),
        },
    }
}

async fn prepare(
    fx: &Fx,
    mgr: &WorkspaceManager,
    run: &str,
) -> Result<Option<PathBuf>, WorkspaceError> {
    let noop = || {};
    mgr.prepare_single_root_workspace(SingleRootPrepare {
        issue_id: "8",
        issue_identifier: "#8",
        run_id: run,
        service_instance_id: "test",
        checkout: &fx.ctx,
        spec: &fx.spec,
        on_progress: &noop,
    })
    .await
    .map(|o| o.map(|l| l.workspace.path.clone()))
}

#[tokio::test]
async fn fresh_workspace_is_checked_out_at_root_and_reused() {
    let fx = fixture();
    let mgr = WorkspaceManager::new(fx.root.clone(), HooksConfig::default());
    let ws = prepare(&fx, &mgr, "r1").await.unwrap().unwrap();

    // The workspace root is the repository: Codex finds AGENTS.md itself.
    assert!(ws.join(".git").is_dir());
    assert_eq!(
        std::fs::read_to_string(ws.join("AGENTS.md")).unwrap(),
        "APP RULES\n"
    );
    assert_eq!(
        git(&ws, &["rev-parse", "--show-toplevel"]),
        ws.canonicalize().unwrap().to_string_lossy()
    );
    // Symphony files are excluded, nothing shows up as untracked.
    assert_eq!(git(&ws, &["status", "--porcelain"]), "");
    let exclude = std::fs::read_to_string(ws.join(".git/info/exclude")).unwrap();
    assert!(exclude.contains("/.symphony-workspace.json"));
    // Full clone (no partial-clone promisor), remote without credentials,
    // and no pre-push hook / repo-level credential override.
    assert_eq!(
        Command::new("git")
            .args(["config", "remote.origin.promisor"])
            .current_dir(&ws)
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
    assert!(!git(&ws, &["remote", "get-url", "origin"]).contains('@'));
    assert!(!ws.join(".git/hooks/pre-push").exists());
    assert_eq!(
        Command::new("git")
            .args(["config", "--local", "credential.helper"])
            .current_dir(&ws)
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        mgr.workspace_layout("8", "#8").await,
        Some(WorkspaceLayout::SingleRoot)
    );
    assert!(
        mgr.has_legacy_workspace("8", "#8").await,
        "single_root counts as single-repo layout"
    );

    // The agent creates its own branch name and pushes it: allowed.
    git(&ws, &["checkout", "-q", "-b", "issue-8-my-fix"]);
    std::fs::write(ws.join("a.txt"), "x").unwrap();
    git(&ws, &["add", "a.txt"]);
    git(&ws, &["commit", "-qm", "fix"]);
    git(&ws, &["push", "-q", "origin", "HEAD"]);

    // Upstream moves; the next run reuses the workspace and refreshes origin/dev.
    let seed = fx.host.parent().unwrap().join("seed");
    std::fs::write(seed.join("NEW"), "n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-qm", "upstream"]);
    git(
        &seed,
        &[
            "push",
            "-q",
            fx.host.join("g/app.git").to_str().unwrap(),
            "HEAD:refs/heads/dev",
        ],
    );
    let ws2 = prepare(&fx, &mgr, "r2").await.unwrap().unwrap();
    assert_eq!(ws, ws2);
    assert_eq!(
        git(&ws, &["rev-parse", "origin/dev"]),
        git(&seed, &["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&ws, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "issue-8-my-fix",
        "work branch untouched"
    );
}

#[tokio::test]
async fn interrupted_initialization_is_reentered() {
    let fx = fixture();
    let mgr = WorkspaceManager::new(fx.root.clone(), HooksConfig::default());
    let ws = prepare(&fx, &mgr, "r1").await.unwrap().unwrap();
    // Simulate a crash mid-initialization.
    let marker = ws.join(".symphony-workspace.json");
    let text = std::fs::read_to_string(&marker)
        .unwrap()
        .replace("\"ready\"", "\"initializing\"");
    std::fs::write(&marker, text).unwrap();
    std::fs::remove_file(ws.join("AGENTS.md")).unwrap();
    let ws2 = prepare(&fx, &mgr, "r2").await.unwrap().unwrap();
    assert!(ws2.join("AGENTS.md").exists(), "checkout was re-done");
    assert!(std::fs::read_to_string(&marker)
        .unwrap()
        .contains("\"ready\""));
}

#[tokio::test]
async fn legacy_workspaces_are_left_alone_and_marker_only_is_upgraded() {
    let fx = fixture();
    let mgr = WorkspaceManager::new(fx.root.clone(), HooksConfig::default());
    // A schema-1 workspace with an agent-cloned repo/ stays legacy.
    let lease = mgr
        .prepare_issue_workspace("8", "#8", "old", "test")
        .await
        .unwrap();
    let ws = lease.workspace.path.clone();
    drop(lease);
    std::fs::create_dir_all(ws.join("repo")).unwrap();
    git(&ws.join("repo"), &["init", "-q"]);
    assert!(prepare(&fx, &mgr, "r1").await.unwrap().is_none());
    assert_eq!(
        mgr.workspace_layout("8", "#8").await,
        Some(WorkspaceLayout::Legacy)
    );

    // A schema-1 marker with nothing else is upgraded in place.
    std::fs::remove_dir_all(ws.join("repo")).unwrap();
    let ws2 = prepare(&fx, &mgr, "r2").await.unwrap().unwrap();
    assert!(ws2.join("AGENTS.md").exists());
    assert_eq!(
        mgr.workspace_layout("8", "#8").await,
        Some(WorkspaceLayout::SingleRoot)
    );

    // Schema 3 never enters the multi-repo path.
    let ctx = fx.ctx.clone();
    let noop = || {};
    let err = mgr
        .prepare_multi_repo_workspace(symphony_platform::workspace::MultiRepoPrepare {
            issue_id: "8",
            issue_identifier: "#8",
            run_id: "r3",
            service_instance_id: "test",
            checkout: &ctx,
            specs: &[],
            on_progress: &noop,
        })
        .await
        .err()
        .unwrap();
    assert!(
        err.to_string().contains("single_root_not_migrated"),
        "{err}"
    );
}

#[tokio::test]
async fn legacy_ready_workspace_without_git_keeps_running() {
    let fx = fixture();
    let mgr = WorkspaceManager::new(fx.root.clone(), HooksConfig::default());
    let lease = mgr
        .prepare_issue_workspace("8", "#8", "old", "test")
        .await
        .unwrap();
    let ws = lease.workspace.path.clone();
    drop(lease);
    std::fs::write(ws.join("src.ts"), "x").unwrap(); // files, but no .git
    let again = mgr
        .prepare_issue_workspace("8", "#8", "new", "test")
        .await
        .unwrap();
    assert_eq!(again.workspace.path, ws);
    // single_root never adopts such a workspace.
    drop(again);
    assert!(prepare(&fx, &mgr, "r").await.unwrap().is_none());
}

#[tokio::test]
async fn ready_single_root_is_reused_by_worker_without_checkout_config() {
    // The testing agent's workflow carries no `workspace.slug`, so it enters
    // through `prepare_issue_workspace`; a ready single_root must be reused.
    let fx = fixture();
    let mgr = WorkspaceManager::new(fx.root.clone(), HooksConfig::default());
    let ws = prepare(&fx, &mgr, "r1").await.unwrap().unwrap();
    let lease = mgr
        .prepare_issue_workspace("8", "#8", "test-r1", "test")
        .await
        .expect("ready single_root is reusable");
    assert_eq!(lease.workspace.path, ws);
    assert!(lease.workspace.path.join(".git").is_dir());
}
