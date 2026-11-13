//! Approved change documents lookup before turn 1 (P2 design §4 B3).

use std::sync::Arc;

use async_trait::async_trait;

use symphony_platform::agent::change_docs::{doc_target, lookup, DocTarget};
use symphony_platform::agent::runner::{AgentIssue, IssueStateRefresher};
use symphony_platform::config::SingleRepoSpec;
use symphony_platform::platform::{MemoryAdapter, Platform};
use symphony_platform::prompt::{ChangeDocsLookup, RepoContext};
use symphony_platform::tracker::gitlab::GitlabTrackerAdapter;
use symphony_platform::tracker::Tracker;

fn repo(alias: &str, role: &str, path: &str) -> RepoContext {
    RepoContext {
        alias: alias.into(),
        slug: format!("g/{alias}"),
        path: path.into(),
        role: role.into(),
        writable: true,
        branch: None,
        default_branch: "develop".into(),
        merge_after: vec![],
    }
}

#[test]
fn test_doc_target_single_repo_uses_tracker_slug_and_default_branch_ref() {
    let t = doc_target("#42", "42", &[], None, "acme/app").unwrap();
    assert_eq!(
        t,
        DocTarget {
            slug: "acme/app".into(),
            git_ref: String::new(),
            repo_dir: "docs/changes/42".into(),
            prompt_dir: "docs/changes/42".into(),
        }
    );
}

#[test]
fn test_doc_target_single_root_spec_overrides_slug_and_ref() {
    let spec = SingleRepoSpec {
        slug: "grp/svc".into(),
        default_branch: "main".into(),
    };
    let t = doc_target("#7", "7", &[], Some(&spec), "ignored").unwrap();
    assert_eq!(t.slug, "grp/svc");
    assert_eq!(t.git_ref, "main");
    assert_eq!(t.prompt_dir, "docs/changes/7");
}

#[test]
fn test_doc_target_multi_repo_prefixes_primary_path() {
    let repos = vec![
        repo("frontend", "secondary", "frontend"),
        repo("backend", "primary", "backend"),
    ];
    let t = doc_target("#9", "9", &repos, None, "ignored").unwrap();
    assert_eq!(t.slug, "g/backend");
    assert_eq!(t.git_ref, "develop");
    assert_eq!(t.repo_dir, "docs/changes/9");
    assert_eq!(t.prompt_dir, "backend/docs/changes/9");
}

#[test]
fn test_doc_target_needs_numeric_issue_and_slug() {
    assert!(doc_target("ENG-12", "abc-uuid", &[], None, "acme/app").is_none());
    assert!(doc_target("#12", "12", &[], None, "").is_none());
    // Identifier wins; id is only a fallback.
    assert_eq!(
        doc_target("", "15", &[], None, "a/b").unwrap().repo_dir,
        "docs/changes/15"
    );
}

/// Refresher backed by a real tracker adapter over the memory platform.
struct TrackerRefresher(Arc<dyn Tracker>);

#[async_trait]
impl IssueStateRefresher for TrackerRefresher {
    async fn refresh_issue_state(&self, _id: &str) -> Result<Option<AgentIssue>, String> {
        Ok(None)
    }
    fn is_terminal_state(&self, _s: &str) -> bool {
        false
    }
    fn is_active_state(&self, _s: &str) -> bool {
        true
    }
    async fn list_repo_directory(
        &self,
        slug: &str,
        path: &str,
        git_ref: &str,
    ) -> Result<Option<Vec<symphony_platform::platform::RepoFile>>, String> {
        self.0
            .list_repo_directory(slug, path, git_ref)
            .await
            .map_err(|e| e.to_string())
    }
}

struct NoListing;

#[async_trait]
impl IssueStateRefresher for NoListing {
    async fn refresh_issue_state(&self, _id: &str) -> Result<Option<AgentIssue>, String> {
        Ok(None)
    }
    fn is_terminal_state(&self, _s: &str) -> bool {
        false
    }
    fn is_active_state(&self, _s: &str) -> bool {
        true
    }
}

fn target() -> DocTarget {
    doc_target("#42", "42", &[], None, "acme/app").unwrap()
}

#[tokio::test]
async fn test_lookup_present_absent_and_error_via_tracker() {
    let mem = MemoryAdapter::new();
    mem.seed_repo_file("acme/app", "", "docs/changes/42/plan.md", "p1")
        .await;
    mem.seed_repo_file("acme/app", "", "docs/changes/42/spec.md", "s1")
        .await;
    let platform: Arc<dyn Platform> = Arc::new(mem.clone());
    let tracker: Arc<dyn Tracker> = Arc::new(GitlabTrackerAdapter::new(
        platform,
        vec!["Todo".into()],
        vec!["Done".into()],
    ));
    let refresher = TrackerRefresher(tracker);

    match lookup(&refresher, &target()).await {
        ChangeDocsLookup::Present(docs) => {
            assert_eq!(docs.dir, "docs/changes/42");
            assert_eq!(docs.git_ref, "");
            let names: Vec<_> = docs.files.iter().map(|f| f.name.as_str()).collect();
            assert_eq!(names, vec!["plan.md", "spec.md"]);
            assert_eq!(docs.files[0].blob_sha, "p1");
        }
        other => panic!("expected Present, got {other:?}"),
    }

    let missing = doc_target("#43", "43", &[], None, "acme/app").unwrap();
    assert!(matches!(
        lookup(&refresher, &missing).await,
        ChangeDocsLookup::Absent
    ));

    mem.with_fault(
        "list_directory",
        symphony_platform::error::PlatformError::Timeout,
    )
    .await;
    assert!(matches!(
        lookup(&refresher, &target()).await,
        ChangeDocsLookup::Unknown
    ));
}

#[tokio::test]
async fn test_lookup_unsupported_refresher_is_unknown() {
    assert!(matches!(
        lookup(&NoListing, &target()).await,
        ChangeDocsLookup::Unknown
    ));
}
