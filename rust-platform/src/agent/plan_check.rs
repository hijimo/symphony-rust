//! Plan deviation check (P2 design §4 B2).
//!
//! After a development run hands off for review, compare the files changed on
//! each writable repository with the `## Files` section of the issue's
//! `plan.md`, and annotate the PR, workpad and labels. Never blocks.

use crate::agent::post_run::{self, CheckedRepo};
use crate::agent::runner::IssueStateRefresher;
use crate::platform::CommentMode;

pub const MARKER: &str = "<!-- symphony:plan-check -->";
pub const WORKPAD_HEADING: &str = "### Symphony plan check";
pub const LABEL_PLAN_DEVIATION: &str = "plan-deviation";

/// Paths listed under `## Files` (case-insensitive heading). `None` when the
/// section is missing. Entries ending in `/` denote whole directories.
pub fn parse_files_section(plan_md: &str) -> Option<Vec<String>> {
    let mut lines = plan_md.lines();
    lines.find(|l| {
        let t = l.trim();
        t.starts_with("## ") && t[3..].trim().eq_ignore_ascii_case("files")
    })?;
    let mut out = Vec::new();
    for line in lines {
        let t = line.trim();
        if t.starts_with('#') {
            break;
        }
        let Some(item) = list_item(t) else { continue };
        let item = match item.strip_prefix('`') {
            Some(rest) => rest.split('`').next().unwrap_or(""),
            None => item
                .split(" #")
                .next()
                .unwrap_or("")
                .split(" — ")
                .next()
                .unwrap_or(""),
        };
        let item = item.trim().trim_start_matches("./");
        if !item.is_empty() {
            out.push(item.to_string());
        }
    }
    Some(out)
}

fn list_item(t: &str) -> Option<&str> {
    for p in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(p) {
            return Some(rest.trim());
        }
    }
    let digits = t.find(|c: char| !c.is_ascii_digit())?;
    (digits > 0)
        .then(|| t[digits..].strip_prefix(". ").map(str::trim))
        .flatten()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanComparison {
    /// Changed files not covered by any `## Files` entry.
    pub unplanned: Vec<String>,
    /// `## Files` entries that no change touched.
    pub untouched: Vec<String>,
}

impl PlanComparison {
    pub fn is_consistent(&self) -> bool {
        self.unplanned.is_empty() && self.untouched.is_empty()
    }
}

fn covers(entry: &str, file: &str) -> bool {
    if entry.ends_with('/') {
        file.starts_with(entry)
    } else {
        entry == file
    }
}

pub fn compare(changed: &[String], planned: &[String]) -> PlanComparison {
    let mut unplanned: Vec<String> = changed
        .iter()
        .filter(|f| !planned.iter().any(|e| covers(e, f)))
        .cloned()
        .collect();
    let mut untouched: Vec<String> = planned
        .iter()
        .filter(|e| !changed.iter().any(|f| covers(e, f)))
        .cloned()
        .collect();
    unplanned.sort();
    untouched.sort();
    PlanComparison {
        unplanned,
        untouched,
    }
}

/// Markdown report shared by the PR comment and the workpad section.
pub fn report(c: &PlanComparison, plan_path: &str) -> String {
    let list = |v: &[String]| {
        v.iter()
            .map(|f| format!("- `{f}`"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut out = String::from("## Symphony 计划一致性检查\n\n");
    if c.is_consistent() {
        out.push_str(&format!(
            "✅ 改动文件与 `{plan_path}` 的 `## Files` 一致。\n"
        ));
    } else {
        out.push_str(&format!(
            "⚠️ 改动与 `{plan_path}` 的 `## Files` 存在偏离（仅标注，不阻断合并）。\n"
        ));
        if !c.unplanned.is_empty() {
            out.push_str(&format!("\n**计划外的改动**\n{}\n", list(&c.unplanned)));
        }
        if !c.untouched.is_empty() {
            out.push_str(&format!("\n**计划内但未改动**\n{}\n", list(&c.untouched)));
        }
    }
    out.push_str(&format!("\n{MARKER}\n"));
    out
}

pub struct PlanCheckInput<'a> {
    pub issue_id: &'a str,
    pub issue_number: u64,
    pub repos: &'a [CheckedRepo],
    /// Multi-repo workspaces prefix paths with the repository alias.
    pub multi_repo: bool,
    pub primary_alias: &'a str,
    /// Slug of the primary repository on the platform (PR lookup).
    pub primary_slug: &'a str,
}

/// Run the check and write the results back. `None` when there is nothing to
/// check (no `plan.md`, no `## Files`, or the primary repo is unknown).
pub async fn run(
    refresher: &dyn IssueStateRefresher,
    input: &PlanCheckInput<'_>,
) -> Option<PlanComparison> {
    let primary = input
        .repos
        .iter()
        .find(|r| r.alias == input.primary_alias)?;
    let docs_dir = format!("docs/changes/{}", input.issue_number);
    let plan_rel = format!("{docs_dir}/plan.md");
    let plan_md = tokio::fs::read_to_string(primary.dir.join(&plan_rel))
        .await
        .ok()?;
    let Some(planned) = parse_files_section(&plan_md) else {
        tracing::info!(issue_id = %input.issue_id, "plan.md has no ## Files section; skipping plan check");
        return None;
    };

    let prefix = |alias: &str| {
        if input.multi_repo {
            format!("{alias}/")
        } else {
            String::new()
        }
    };
    let excluded = format!("{}{docs_dir}/", prefix(input.primary_alias));
    let mut changed = Vec::new();
    for repo in input.repos {
        let Some(base) = diff_base(repo).await else {
            tracing::warn!(issue_id = %input.issue_id, alias = %repo.alias, "default branch unknown; skipping plan check");
            return None;
        };
        let range = format!("{base}...HEAD");
        match git_lines(&repo.dir, &["diff", "--name-only", &range]).await {
            Some(files) => changed.extend(
                files
                    .into_iter()
                    .map(|f| format!("{}{f}", prefix(&repo.alias)))
                    .filter(|f| !f.starts_with(&excluded)),
            ),
            None => {
                tracing::warn!(issue_id = %input.issue_id, alias = %repo.alias, "git diff failed; skipping plan check");
                return None;
            }
        }
    }
    let comparison = compare(&changed, &planned);
    let plan_path = format!("{}{plan_rel}", prefix(input.primary_alias));
    let text = report(&comparison, &plan_path);
    write_back(refresher, input, primary, &comparison, &text).await;
    Some(comparison)
}

async fn write_back(
    refresher: &dyn IssueStateRefresher,
    input: &PlanCheckInput<'_>,
    primary: &CheckedRepo,
    comparison: &PlanComparison,
    text: &str,
) {
    let label = [LABEL_PLAN_DEVIATION.to_string()];
    let labels = if comparison.is_consistent() {
        refresher.remove_labels(input.issue_id, &label).await
    } else {
        refresher.add_labels(input.issue_id, &label).await
    };
    if let Err(e) = labels {
        tracing::warn!(issue_id = %input.issue_id, error = %e, "plan check: label update failed");
    }

    if let Ok(Some((comment_id, body))) = refresher.fetch_workpad(input.issue_id).await {
        let section = text.replace(MARKER, "").trim().to_string();
        if let Some(updated) = post_run::with_section(&body, WORKPAD_HEADING, &section) {
            let _ = refresher.update_workpad(comment_id, &updated).await;
        }
    }

    let Some(branch) = git_lines(&primary.dir, &["symbolic-ref", "--short", "-q", "HEAD"])
        .await
        .and_then(|l| l.into_iter().next())
    else {
        return;
    };
    match refresher
        .find_open_pull_request(input.primary_slug, &branch)
        .await
    {
        Ok(Some(pr)) => {
            if let Err(e) = refresher
                .upsert_pull_request_comment(&pr, MARKER, text, CommentMode::Upsert)
                .await
            {
                tracing::warn!(issue_id = %input.issue_id, error = %e, "plan check: PR comment failed");
            }
        }
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(issue_id = %input.issue_id, error = %e, "plan check: PR lookup failed")
        }
    }
}

/// `origin/<default>`; when Symphony does not know the default branch
/// (legacy layout), use the clone's `origin/HEAD`.
async fn diff_base(repo: &CheckedRepo) -> Option<String> {
    if !repo.default_branch.is_empty() {
        return Some(format!("origin/{}", repo.default_branch));
    }
    git_lines(
        &repo.dir,
        &["symbolic-ref", "--short", "-q", "refs/remotes/origin/HEAD"],
    )
    .await
    .and_then(|l| l.into_iter().next())
}

async fn git_lines(dir: &std::path::Path, args: &[&str]) -> Option<Vec<String>> {
    post_run::git_out(dir, args).await.map(|out| {
        out.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    })
}
