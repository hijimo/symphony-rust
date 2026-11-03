//! Workspace root guard (self-verification design §4.1).
//!
//! Codex merges every `AGENTS.md` from the git root down to its `cwd`. A
//! workspace root inside another git repository (for example the Symphony
//! checkout) makes every session load that repository's instructions.

use std::path::{Path, PathBuf};

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

/// Refuse when `root` (or its nearest existing ancestor) is inside a git
/// work tree, or when git cannot tell.
pub async fn ensure_outside_git(root: &Path, what: &str) -> Result<(), String> {
    let abs = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(root))
            .unwrap_or_else(|_| root.to_path_buf())
    };
    let Some(dir) = nearest_existing(&abs) else {
        return Err(format!("{what} {}: no existing ancestor", abs.display()));
    };
    let out = tokio::process::Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree", "--show-toplevel"])
        .current_dir(&dir)
        .env("LC_ALL", "C")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .await
        .map_err(|e| format!("cannot verify {what}: failed to run git: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if out.status.success() {
        let top = stdout.lines().nth(1).unwrap_or("").trim().to_string();
        return Err(format!(
            "{what} {} is inside the git repository {top}; Codex would load that repository's \
             AGENTS.md and skills for every session. Move it outside any git repository \
             (for example ~/symphony-workspaces)",
            abs.display()
        ));
    }
    if out.status.code() == Some(128) && stderr.contains("not a git repository") {
        return Ok(());
    }
    Err(format!(
        "cannot verify that {what} {} is outside any git repository: {}",
        abs.display(),
        stderr.trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_inside_repo_and_accepts_outside() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("a/b");
        assert!(ensure_outside_git(&outside, "root").await.is_ok());
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(tmp.path())
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let err = ensure_outside_git(&outside, "root").await.unwrap_err();
        assert!(err.contains("inside the git repository"), "{err}");
    }
}
