//! Approved change documents lookup (P2 design §4 B3).
//!
//! Before the first turn the runner lists `docs/changes/<iid>/` on the
//! primary repository's default branch through the tracker, so the prompt
//! can name the approved documents and their blob SHAs. The lookup goes
//! through the platform API, so it works for every workspace layout
//! (including legacy layouts where the agent clones the repository itself).

use crate::agent::runner::IssueStateRefresher;
use crate::config::SingleRepoSpec;
use crate::prompt::{ChangeDocFile, ChangeDocs, ChangeDocsLookup, RepoContext};

/// Where to look for the documents of one issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocTarget {
    /// Repository slug passed to the platform API.
    pub slug: String,
    /// Ref to read; empty means the repository default branch.
    pub git_ref: String,
    /// Directory relative to the repository root.
    pub repo_dir: String,
    /// Directory relative to the workspace root (shown in the prompt).
    pub prompt_dir: String,
}

/// Issue number from `#42` (preferred) or a numeric id.
fn issue_number(identifier: &str, id: &str) -> Option<u64> {
    let ident = identifier.trim().trim_start_matches('#');
    ident
        .parse::<u64>()
        .ok()
        .or_else(|| id.trim().parse::<u64>().ok())
}

/// Resolve the repository and directory to inspect. `None` when the issue has
/// no numeric number (e.g. Linear) or no repository slug is known.
pub fn doc_target(
    identifier: &str,
    id: &str,
    repos: &[RepoContext],
    single: Option<&SingleRepoSpec>,
    tracker_slug: &str,
) -> Option<DocTarget> {
    let number = issue_number(identifier, id)?;
    let repo_dir = format!("docs/changes/{number}");
    let primary = repos
        .iter()
        .find(|r| r.role == "primary")
        .or_else(|| repos.first());
    let (slug, git_ref, prompt_dir) = match (primary, single) {
        (Some(p), _) => (
            p.slug.clone(),
            p.default_branch.clone(),
            format!("{}/{repo_dir}", p.path.trim_end_matches('/')),
        ),
        (None, Some(spec)) => (
            spec.slug.clone(),
            spec.default_branch.clone(),
            repo_dir.clone(),
        ),
        (None, None) => (tracker_slug.to_string(), String::new(), repo_dir.clone()),
    };
    if slug.trim().is_empty() {
        return None;
    }
    Some(DocTarget {
        slug,
        git_ref,
        repo_dir,
        prompt_dir,
    })
}

/// Look the documents up. Errors and unsupported trackers yield `Unknown`
/// so the prompt falls back to generic instructions instead of failing.
pub async fn lookup(refresher: &dyn IssueStateRefresher, target: &DocTarget) -> ChangeDocsLookup {
    match refresher
        .list_repo_directory(&target.slug, &target.repo_dir, &target.git_ref)
        .await
    {
        Ok(Some(files)) => ChangeDocsLookup::Present(ChangeDocs {
            dir: target.prompt_dir.clone(),
            git_ref: target.git_ref.clone(),
            files: files
                .into_iter()
                .map(|f| ChangeDocFile {
                    name: f.name,
                    blob_sha: f.blob_sha,
                })
                .collect(),
        }),
        Ok(None) => ChangeDocsLookup::Absent,
        Err(error) => {
            tracing::warn!(slug = %target.slug, dir = %target.repo_dir, %error, "change docs lookup failed");
            ChangeDocsLookup::Unknown
        }
    }
}
