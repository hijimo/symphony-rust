//! Plan deviation check: diff vs `plan.md ## Files` (P2 design §4 B2).

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

use async_trait::async_trait;
use tempfile::TempDir;

use symphony_platform::agent::plan_check::{
    compare, parse_files_section, report, run, PlanCheckInput, LABEL_PLAN_DEVIATION, MARKER,
    WORKPAD_HEADING,
};
use symphony_platform::agent::post_run::CheckedRepo;
use symphony_platform::agent::runner::{AgentIssue, IssueStateRefresher};
use symphony_platform::platform::{CommentMode, PullRequestRef};

// ---------------------------------------------------------------- parsing

#[test]
fn test_parse_files_section_accepts_list_styles_and_stops_at_next_heading() {
    let md = "# Plan\n\n## Steps\n- not a file\n\n## Files\n\n- backend/src/a.rs\n* `frontend/src/pages/` — the page\n+ ./docs/readme.md # comment\n1. tests/x_test.rs\n- `with space/file.md`\n-\n\nfree text line\n## Tests\n- tests/ignored.rs\n";
    assert_eq!(
        parse_files_section(md),
        Some(vec![
            "backend/src/a.rs".to_string(),
            "frontend/src/pages/".to_string(),
            "docs/readme.md".to_string(),
            "tests/x_test.rs".to_string(),
            "with space/file.md".to_string(),
        ])
    );
}

#[test]
fn test_parse_files_section_missing_heading_is_none() {
    assert_eq!(parse_files_section("# Plan\n## Steps\n- a\n"), None);
    assert_eq!(
        parse_files_section("## files\n- a.rs\n"),
        Some(vec!["a.rs".into()])
    );
}

#[test]
fn test_compare_exact_paths_and_directory_entries() {
    let changed = vec![
        "src/a.rs".to_string(),
        "src/pages/list.tsx".to_string(),
        "src/pages/detail.tsx".to_string(),
        "Cargo.lock".to_string(),
    ];
    let planned = vec![
        "src/a.rs".to_string(),
        "src/pages/".to_string(),
        "src/b.rs".to_string(),
    ];
    let c = compare(&changed, &planned);
    assert_eq!(c.unplanned, vec!["Cargo.lock".to_string()]);
    assert_eq!(c.untouched, vec!["src/b.rs".to_string()]);
    assert!(!c.is_consistent());

    let ok = compare(&["src/a.rs".to_string()], &["src/a.rs".to_string()]);
    assert!(ok.is_consistent());
}

#[test]
fn test_compare_directory_entry_does_not_match_sibling_prefix() {
    let c = compare(
        &["src/pagesx/a.rs".to_string()],
        &["src/pages/".to_string()],
    );
    assert_eq!(c.unplanned, vec!["src/pagesx/a.rs".to_string()]);
    assert_eq!(c.untouched, vec!["src/pages/".to_string()]);
}

#[test]
fn test_report_contains_marker_and_lists() {
    let c = compare(&["x.rs".to_string()], &["y.rs".to_string()]);
    let text = report(&c, "docs/changes/7/plan.md");
    assert!(text.contains(MARKER));
    assert!(text.contains("偏离"));
    assert!(text.contains("- `x.rs`"));
    assert!(text.contains("- `y.rs`"));
    let ok = report(&compare(&[], &[]), "docs/changes/7/plan.md");
    assert!(ok.contains("一致"));
}

// ---------------------------------------------------------------- run()

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

fn write(dir: &Path, rel: &str, body: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

/// A clone of a bare remote with `main`, then a feature branch with `files`
/// committed on top. Returns the clone directory.
fn repo_with_changes(
    tmp: &Path,
    name: &str,
    base_files: &[(&str, &str)],
    files: &[(&str, &str)],
) -> std::path::PathBuf {
    let bare = tmp.join(format!("{name}.git"));
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare", "-q", "--initial-branch=main"]);
    let clone = tmp.join(name);
    std::fs::create_dir_all(&clone).unwrap();
    git(&clone, &["init", "-q", "--initial-branch=main"]);
    write(&clone, "README.md", "x");
    for (p, b) in base_files {
        write(&clone, p, b);
    }
    git(&clone, &["add", "."]);
    git(&clone, &["commit", "-qm", "init"]);
    git(&clone, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&clone, &["push", "-q", "origin", "main"]);
    git(&clone, &["fetch", "-q", "origin"]);
    git(&clone, &["checkout", "-qb", "feat/x"]);
    for (p, b) in files {
        write(&clone, p, b);
    }
    git(&clone, &["add", "."]);
    git(&clone, &["commit", "-qm", "work"]);
    clone
}

#[derive(Default)]
struct Fake {
    workpad: Mutex<String>,
    added: Mutex<Vec<String>>,
    removed: Mutex<Vec<String>>,
    pr_lookups: Mutex<Vec<(String, String)>>,
    comments: Mutex<Vec<(PullRequestRef, String, String, CommentMode)>>,
    has_pr: bool,
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
        *self.workpad.lock().unwrap() = body.to_string();
        Ok(())
    }
    async fn add_labels(&self, _id: &str, l: &[String]) -> Result<(), String> {
        self.added.lock().unwrap().extend(l.iter().cloned());
        Ok(())
    }
    async fn remove_labels(&self, _id: &str, l: &[String]) -> Result<(), String> {
        self.removed.lock().unwrap().extend(l.iter().cloned());
        Ok(())
    }
    async fn find_open_pull_request(
        &self,
        slug: &str,
        branch: &str,
    ) -> Result<Option<PullRequestRef>, String> {
        self.pr_lookups
            .lock()
            .unwrap()
            .push((slug.into(), branch.into()));
        Ok(self.has_pr.then(|| PullRequestRef {
            repo_slug: slug.into(),
            number: 5,
            url: "u".into(),
        }))
    }
    async fn upsert_pull_request_comment(
        &self,
        pr: &PullRequestRef,
        marker: &str,
        body: &str,
        mode: CommentMode,
    ) -> Result<(), String> {
        self.comments
            .lock()
            .unwrap()
            .push((pr.clone(), marker.into(), body.into(), mode));
        Ok(())
    }
}

const PLAN: &str = "# Plan\n## Files\n- src/a.rs\n- src/b.rs\n";

#[tokio::test]
async fn test_run_single_repo_reports_deviation_everywhere() {
    let tmp = TempDir::new().unwrap();
    let dir = repo_with_changes(
        tmp.path(),
        "app",
        &[],
        &[
            ("src/a.rs", "1"),
            ("src/extra.rs", "1"),
            ("docs/changes/7/plan.md", PLAN),
        ],
    );
    let repos = vec![CheckedRepo {
        alias: "primary".into(),
        dir: dir.clone(),
        default_branch: "main".into(),
    }];
    let fake = Fake {
        workpad: Mutex::new("## Codex Workpad\nbody\n".into()),
        has_pr: true,
        ..Default::default()
    };
    let out = run(
        &fake,
        &PlanCheckInput {
            issue_id: "7",
            issue_number: 7,
            repos: &repos,
            multi_repo: false,
            primary_alias: "primary",
            primary_slug: "acme/app",
        },
    )
    .await
    .expect("checked");

    assert_eq!(out.unplanned, vec!["src/extra.rs".to_string()]);
    assert_eq!(out.untouched, vec!["src/b.rs".to_string()]);
    assert_eq!(
        fake.added.lock().unwrap().as_slice(),
        &[LABEL_PLAN_DEVIATION.to_string()]
    );
    assert!(fake.workpad.lock().unwrap().contains(WORKPAD_HEADING));
    assert_eq!(
        fake.pr_lookups.lock().unwrap().as_slice(),
        &[("acme/app".to_string(), "feat/x".to_string())]
    );
    let comments = fake.comments.lock().unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].1, MARKER);
    assert_eq!(comments[0].3, CommentMode::Upsert);
    assert!(comments[0].2.contains("src/extra.rs"));
}

#[tokio::test]
async fn test_run_consistent_removes_label_and_tolerates_missing_pr() {
    let tmp = TempDir::new().unwrap();
    // plan.md already on main; only planned files change on the branch.
    let dir = repo_with_changes(
        tmp.path(),
        "app",
        &[("docs/changes/7/plan.md", PLAN)],
        &[("src/a.rs", "1"), ("src/b.rs", "2")],
    );
    let repos = vec![CheckedRepo {
        alias: "primary".into(),
        dir,
        default_branch: "main".into(),
    }];
    let fake = Fake::default();
    let out = run(
        &fake,
        &PlanCheckInput {
            issue_id: "7",
            issue_number: 7,
            repos: &repos,
            multi_repo: false,
            primary_alias: "primary",
            primary_slug: "acme/app",
        },
    )
    .await
    .expect("checked");
    assert!(out.is_consistent());
    assert_eq!(
        fake.removed.lock().unwrap().as_slice(),
        &[LABEL_PLAN_DEVIATION.to_string()]
    );
    assert!(fake.added.lock().unwrap().is_empty());
    assert!(fake.comments.lock().unwrap().is_empty(), "no PR yet");
}

#[tokio::test]
async fn test_run_multi_repo_prefixes_aliases_and_reads_primary_plan() {
    let tmp = TempDir::new().unwrap();
    let plan = "## Files\n- backend/src/api.rs\n- frontend/src/\n";
    let backend = repo_with_changes(
        tmp.path(),
        "backend",
        &[],
        &[("src/api.rs", "1"), ("docs/changes/9/plan.md", plan)],
    );
    let frontend = repo_with_changes(tmp.path(), "frontend", &[], &[("src/page.tsx", "1")]);
    let repos = vec![
        CheckedRepo {
            alias: "frontend".into(),
            dir: frontend,
            default_branch: "main".into(),
        },
        CheckedRepo {
            alias: "backend".into(),
            dir: backend,
            default_branch: "main".into(),
        },
    ];
    let fake = Fake::default();
    let out = run(
        &fake,
        &PlanCheckInput {
            issue_id: "9",
            issue_number: 9,
            repos: &repos,
            multi_repo: true,
            primary_alias: "backend",
            primary_slug: "g/backend",
        },
    )
    .await
    .expect("checked");
    assert!(out.is_consistent(), "{out:?}");
    assert_eq!(fake.pr_lookups.lock().unwrap()[0].0, "g/backend");
}

#[tokio::test]
async fn test_run_without_plan_md_does_nothing() {
    let tmp = TempDir::new().unwrap();
    let dir = repo_with_changes(tmp.path(), "app", &[], &[("src/a.rs", "1")]);
    let repos = vec![CheckedRepo {
        alias: "primary".into(),
        dir,
        default_branch: "main".into(),
    }];
    let fake = Fake::default();
    let out = run(
        &fake,
        &PlanCheckInput {
            issue_id: "7",
            issue_number: 7,
            repos: &repos,
            multi_repo: false,
            primary_alias: "primary",
            primary_slug: "acme/app",
        },
    )
    .await;
    assert!(out.is_none());
    assert!(fake.added.lock().unwrap().is_empty());
    assert!(fake.removed.lock().unwrap().is_empty());
    assert!(fake.pr_lookups.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_run_resolves_empty_default_branch_from_origin_head() {
    // Legacy layout (GitHub single repo): the agent cloned the repository,
    // Symphony does not know the default branch.
    let tmp = TempDir::new().unwrap();
    let dir = repo_with_changes(
        tmp.path(),
        "app",
        &[("docs/changes/7/plan.md", PLAN)],
        &[("src/a.rs", "1"), ("src/c.rs", "1")],
    );
    git(&dir, &["remote", "set-head", "origin", "main"]);
    let repos = vec![CheckedRepo {
        alias: "primary".into(),
        dir,
        default_branch: String::new(),
    }];
    let fake = Fake::default();
    let out = run(
        &fake,
        &PlanCheckInput {
            issue_id: "7",
            issue_number: 7,
            repos: &repos,
            multi_repo: false,
            primary_alias: "primary",
            primary_slug: "acme/app",
        },
    )
    .await
    .expect("default branch resolved from origin/HEAD");
    assert_eq!(out.unplanned, vec!["src/c.rs".to_string()]);
    assert_eq!(out.untouched, vec!["src/b.rs".to_string()]);
}
