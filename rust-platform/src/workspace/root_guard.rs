//! Workspace root guard (self-verification design §4.1).
//!
//! Codex merges every `AGENTS.md` from the git root down to its `cwd`. When
//! the workspace root lives inside another git repository (for example the
//! Symphony checkout itself), every session silently loads that repository's
//! instructions instead of the target project's. Refuse to start in that case.

use std::path::{Path, PathBuf};

use crate::proxy::proxy_command;

/// Result of probing a directory with git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitProbe {
    /// Not inside any git work tree.
    Outside,
    /// Inside the work tree whose top level is the given path.
    Inside(PathBuf),
    /// git could not answer (not installed, safe.directory, ...).
    Error(String),
}

/// Nearest existing ancestor of `path` (the path itself when it exists).
fn nearest_existing(path: &Path) -> Option<PathBuf> {
    let mut cur = Some(path);
    while let Some(p) = cur {
        if p.exists() {
            return Some(p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));
        }
        cur = p.parent();
    }
    None
}

/// Probe `path` (or its nearest existing ancestor) with
/// `git rev-parse --is-inside-work-tree` under `LC_ALL=C`.
pub async fn probe(path: &Path) -> GitProbe {
    let Some(dir) = nearest_existing(path) else {
        return GitProbe::Error(format!("no existing ancestor of {}", path.display()));
    };
    let out = match proxy_command("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(&dir)
        .env("LC_ALL", "C")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => return GitProbe::Error(format!("failed to run git: {e}")),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if out.status.success() {
        if stdout.trim() == "true" {
            let top = proxy_command("git")
                .args(["rev-parse", "--show-toplevel"])
                .current_dir(&dir)
                .env("LC_ALL", "C")
                .output()
                .await
                .ok()
                .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
                .unwrap_or_else(|| dir.clone());
            return GitProbe::Inside(top);
        }
        // Inside a .git directory or a bare repository.
        return GitProbe::Inside(dir);
    }
    if out.status.code() == Some(128) && stderr.contains("not a git repository") {
        return GitProbe::Outside;
    }
    GitProbe::Error(stderr.trim().to_string())
}

/// Ensure `root` is not inside a git work tree. Returns a human readable
/// error otherwise.
pub async fn ensure_outside_git(root: &Path, what: &str) -> Result<(), String> {
    match probe(root).await {
        GitProbe::Outside => Ok(()),
        GitProbe::Inside(top) => Err(format!(
            "{what} {} is inside the git repository {}; Codex would load that repository's \
             AGENTS.md and skills for every session. Move it outside any git repository \
             (for example ~/symphony-workspaces) and update SYMPHONY_WORKSPACE_ROOT / workspace.root",
            root.display(),
            top.display()
        )),
        GitProbe::Error(e) => Err(format!(
            "cannot verify that {what} {} is outside any git repository: {e}",
            root.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) -> String {
        crate::proxy::test_support::test_git(dir, args)
    }

    #[tokio::test]
    async fn outside_any_repo_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ws").join("not-yet-created");
        assert_eq!(probe(&root).await, GitProbe::Outside);
        assert!(ensure_outside_git(&root, "workspace root").await.is_ok());
    }

    #[tokio::test]
    async fn inside_repo_is_rejected_even_when_root_does_not_exist() {
        let tmp = tempfile::tempdir().unwrap();
        git(tmp.path(), &["init", "-q"]);
        let root = tmp.path().join("workspaces").join("7");
        match probe(&root).await {
            GitProbe::Inside(top) => {
                assert_eq!(
                    top.canonicalize().unwrap(),
                    tmp.path().canonicalize().unwrap()
                )
            }
            other => panic!("expected Inside, got {other:?}"),
        }
        let err = ensure_outside_git(&root, "workspace root")
            .await
            .unwrap_err();
        assert!(err.contains("inside the git repository"), "{err}");
    }
}
