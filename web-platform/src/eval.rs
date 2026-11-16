//! `symphony-eval` (P3 design §5 E1): run a fixed set of drafting tasks
//! against an organization rules checkout and compare the pass rate with a
//! baseline. No database and no platform access: each task drafts once in a
//! temporary git workspace with the organization templates (O3) and the
//! CODEX_HOME built from the same export and filters as the server (O2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::sdlc::agent::{AgentConfig, AgentManager, Mode, SessionSpec};
use crate::sdlc::content;
use crate::sdlc::org_config;
use crate::sdlc::service;
use crate::sdlc::templates::{self, OrgTemplates, TemplateKey};
use crate::sdlc::workspace::WorkspaceService;

/// Issue number used for the temporary documents (`docs/changes/0/`).
const EVAL_IID: i64 = 0;
pub const DEFAULT_TOLERANCE: f64 = 5.0;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Expect {
    /// Regular expressions that must all match the document.
    #[serde(default)]
    pub must_include: Vec<String>,
    /// Regular expressions that must not match.
    #[serde(default)]
    pub must_not_include: Vec<String>,
    pub max_total_tokens: Option<i64>,
}

/// `evals/tasks/<name>.yaml`.
#[derive(Debug, Clone, Deserialize)]
pub struct Task {
    #[serde(skip)]
    pub name: String,
    /// `intent`, `spec`, `plan` or `intent.incident`.
    pub doc: String,
    pub requirement: String,
    /// Upstream documents placed next to the draft (`intent.md`, ...).
    #[serde(default)]
    pub context: BTreeMap<String, String>,
    #[serde(default)]
    pub expect: Expect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub passed: bool,
    /// Why the run failed (empty when passed).
    pub reasons: Vec<String>,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskResult {
    pub name: String,
    pub doc: String,
    /// Every run passed.
    pub passed: bool,
    pub runs: Vec<RunResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub org_commit: String,
    pub codex_version: String,
    /// Passed runs / all runs, in percent.
    pub pass_rate: f64,
    pub tasks: Vec<TaskResult>,
}

pub struct Options {
    pub org_config: PathBuf,
    pub codex: String,
    pub repeat: usize,
    pub turn_timeout: Duration,
    /// The deployment CODEX_HOME (`auth.json`, `symphony-*` skills).
    pub base_home: PathBuf,
}

/// Load `*.yaml` / `*.yml` task files sorted by name.
pub fn load_tasks(dir: &Path) -> Result<Vec<Task>, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("无法读取任务目录 {}：{e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "yaml" || x == "yml"))
        .collect();
    paths.sort();
    let mut tasks = Vec::new();
    for p in paths {
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}：{e}", p.display()))?;
        let mut t: Task =
            serde_yaml::from_str(&text).map_err(|e| format!("{}：{e}", p.display()))?;
        TemplateKey::parse(&t.doc).map_err(|e| format!("{}：{e}", p.display()))?;
        for name in t.context.keys() {
            if !valid_context_name(name) {
                return Err(format!("{}：context 文件名无效：{name}", p.display()));
            }
        }
        for re in t
            .expect
            .must_include
            .iter()
            .chain(&t.expect.must_not_include)
        {
            regex::Regex::new(re).map_err(|e| format!("{}：正则无效 {re}：{e}", p.display()))?;
        }
        t.name = p
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        tasks.push(t);
    }
    if tasks.is_empty() {
        return Err(format!("{} 中没有任务", dir.display()));
    }
    Ok(tasks)
}

/// A plain file name (`intent.md`), no directories.
fn valid_context_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// Problems of a drafted document (empty = pass).
pub fn score(
    key: TemplateKey,
    text: &str,
    expect: &Expect,
    total_tokens: i64,
    outcome_error: Option<String>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(e) = outcome_error {
        reasons.push(format!("起草失败：{e}"));
    }
    for i in content::validate(key.doc, text, &[]) {
        reasons.push(format!("校验：## {} {}", i.section, i.message));
    }
    for re in &expect.must_include {
        if !regex::Regex::new(re).is_ok_and(|r| r.is_match(text)) {
            reasons.push(format!("缺少：{re}"));
        }
    }
    for re in &expect.must_not_include {
        if regex::Regex::new(re).is_ok_and(|r| r.is_match(text)) {
            reasons.push(format!("不应包含：{re}"));
        }
    }
    if let Some(max) = expect.max_total_tokens {
        if total_tokens > max {
            reasons.push(format!("token 用量 {total_tokens} 超过 {max}"));
        }
    }
    reasons
}

/// Regressions against a baseline: pass rate down by more than `tolerance`
/// percentage points, or a task that passed in the baseline now fails.
pub fn compare(report: &Report, baseline: &Report, tolerance: f64) -> Vec<String> {
    let mut out = Vec::new();
    if report.pass_rate + tolerance < baseline.pass_rate {
        out.push(format!(
            "通过率 {:.1}% 比基线 {:.1}% 低超过 {tolerance} 个百分点",
            report.pass_rate, baseline.pass_rate
        ));
    }
    for b in baseline.tasks.iter().filter(|t| t.passed) {
        if report.tasks.iter().any(|t| t.name == b.name && !t.passed) {
            out.push(format!("基线中通过的任务 {} 失败", b.name));
        }
    }
    out
}

async fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = tokio::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .await
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `<program> --version` of the Codex command, `unknown` when it fails.
async fn codex_version(codex: &str) -> String {
    let program = codex.split_whitespace().next().unwrap_or("codex");
    match tokio::process::Command::new(program)
        .arg("--version")
        .output()
        .await
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    }
}

/// Run every task `repeat` times.
pub async fn run(opts: &Options, tasks: &[Task]) -> Result<Report, String> {
    let work = tempfile::TempDir::new().map_err(|e| e.to_string())?;
    let org_root = work.path().join("org-config");
    let sha = git(&opts.org_config, &["rev-parse", "HEAD"]).await?;
    let git_dir = PathBuf::from(git(&opts.org_config, &["rev-parse", "--absolute-git-dir"]).await?);
    let workspaces = WorkspaceService::new(work.path().to_str().unwrap_or("."));
    org_config::export_commit(&workspaces, &org_root, &git_dir, &sha).await?;
    let home = org_config::build_home(&org_root, &sha, &opts.base_home)
        .map_err(|e| format!("无法生成 CODEX_HOME：{e}"))?;
    let org = OrgTemplates {
        dir: org_root.join(&sha).join("templates"),
        sha: sha.clone(),
    };

    let agents = AgentManager::new(
        AgentConfig {
            command: opts.codex.clone(),
            turn_timeout: opts.turn_timeout,
            ..AgentConfig::default()
        },
        1,
    );
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    agents.set_usage_sink(tx);

    let mut results = Vec::new();
    let mut flow_id = 0i64;
    for task in tasks {
        let key = TemplateKey::parse(&task.doc).map_err(|e| e.to_string())?;
        let tpl = templates::resolve(key, None, Some(&org));
        let mut runs = Vec::new();
        for _ in 0..opts.repeat.max(1) {
            flow_id -= 1;
            let ws = work.path().join(format!("task{}", -flow_id));
            let writable = format!("docs/changes/{EVAL_IID}");
            let dir = ws.join(&writable);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            git(&ws, &["init", "-q"]).await?;
            for (name, text) in &task.context {
                std::fs::write(dir.join(name), text).map_err(|e| e.to_string())?;
            }
            let doc_path = dir.join(format!("{}.md", key.doc));
            let skeleton = format!(
                "{}{}",
                content::frontmatter(key.doc, EVAL_IID, &tpl.template_ref, &sha, ""),
                templates::render_skeleton(&tpl, "eval")
            );
            std::fs::write(&doc_path, skeleton).map_err(|e| e.to_string())?;

            let session = agents.get_or_create(SessionSpec {
                flow_id,
                stage: key.doc,
                cwd: ws.clone(),
                writable_rel: writable.clone(),
                developer_instructions: service::developer_instructions(&writable, key.doc),
                thread_id: None,
                first_turn_prefix: None,
            });
            session.set_codex_home(Some(home.clone()));
            session.try_begin(Mode::NonInteractive);
            let prompt = format!(
                "{}\n\nRequirement to document:\n{}",
                service::draft_prompt(key.doc, &writable, None, &tpl.prompt),
                task.requirement
            );
            let outcome = session.run_turn(&prompt).await;
            session.end();
            agents.remove_flow(flow_id).await;

            let mut tokens = 0;
            while let Ok(u) = rx.try_recv() {
                if u.flow_id == flow_id {
                    tokens += u.total;
                }
            }
            let text = std::fs::read_to_string(&doc_path).unwrap_or_default();
            let error = (!outcome.is_ok()).then(|| {
                outcome
                    .error
                    .clone()
                    .unwrap_or_else(|| outcome.status.clone())
            });
            let reasons = score(key, &text, &task.expect, tokens, error);
            runs.push(RunResult {
                passed: reasons.is_empty(),
                reasons,
                total_tokens: tokens,
            });
        }
        results.push(TaskResult {
            name: task.name.clone(),
            doc: task.doc.clone(),
            passed: runs.iter().all(|r| r.passed),
            runs,
        });
    }
    let all: Vec<&RunResult> = results.iter().flat_map(|t| &t.runs).collect();
    let pass_rate = if all.is_empty() {
        0.0
    } else {
        all.iter().filter(|r| r.passed).count() as f64 * 100.0 / all.len() as f64
    };
    Ok(Report {
        org_commit: sha,
        codex_version: codex_version(&opts.codex).await,
        pass_rate,
        tasks: results,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdlc::model::DocType;

    fn report(rate: f64, tasks: &[(&str, bool)]) -> Report {
        Report {
            org_commit: "c".into(),
            codex_version: "v".into(),
            pass_rate: rate,
            tasks: tasks
                .iter()
                .map(|(n, p)| TaskResult {
                    name: n.to_string(),
                    doc: "spec".into(),
                    passed: *p,
                    runs: vec![],
                })
                .collect(),
        }
    }

    #[test]
    fn baseline_comparison_flags_rate_drops_and_newly_failing_tasks() {
        let base = report(100.0, &[("a", true), ("b", true), ("c", false)]);
        assert!(compare(
            &report(100.0, &[("a", true), ("b", true), ("c", true)]),
            &base,
            5.0
        )
        .is_empty());
        let r = compare(
            &report(96.0, &[("a", true), ("b", true), ("c", false)]),
            &base,
            5.0,
        );
        assert!(r.is_empty(), "within tolerance: {r:?}");
        let r = compare(
            &report(66.7, &[("a", true), ("b", false), ("c", false)]),
            &base,
            5.0,
        );
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(r[0].contains("通过率") && r[1].contains("任务 b"));
        // A task missing from the new report is not a regression by itself.
        assert!(compare(
            &report(100.0, &[("a", true)]),
            &report(100.0, &[("a", true), ("z", true)]),
            0.0
        )
        .is_empty());
    }

    #[test]
    fn scoring_combines_validation_expectations_and_tokens() {
        let spec = "# Spec\n\n## Requirements\n鉴权 R1\n\n## Design\nD1\n\n## Concerns\n- PII. owner: sec. resolution: masked\n";
        let expect = Expect {
            must_include: vec!["鉴权".into(), "owner: \\w+".into()],
            must_not_include: vec!["TODO".into()],
            max_total_tokens: Some(100),
        };
        assert!(score(DocType::Spec.into(), spec, &expect, 50, None).is_empty());
        let r = score(
            DocType::Spec.into(),
            "# Spec\n\n## Requirements\nTODO\n",
            &expect,
            500,
            Some("boom".into()),
        );
        assert!(r.iter().any(|x| x.contains("起草失败：boom")), "{r:?}");
        assert!(r.iter().any(|x| x.starts_with("校验：## Design")), "{r:?}");
        assert!(r.iter().any(|x| x == "缺少：鉴权"), "{r:?}");
        assert!(r.iter().any(|x| x == "不应包含：TODO"), "{r:?}");
        assert!(
            r.iter().any(|x| x.contains("token 用量 500 超过 100")),
            "{r:?}"
        );
    }

    #[test]
    fn tasks_load_sorted_and_invalid_ones_are_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let w = |n: &str, t: &str| std::fs::write(dir.path().join(n), t).unwrap();
        w("b.yaml", "doc: plan\nrequirement: export\n");
        w(
            "a.yml",
            "doc: intent.incident\nrequirement: 5xx\ncontext:\n  intent.md: x\nexpect:\n  must_include: [\"a\"]\n",
        );
        w("notes.md", "ignored");
        let t = load_tasks(dir.path()).unwrap();
        assert_eq!(
            t.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(t[0].context["intent.md"], "x");
        assert_eq!(t[0].expect.must_include, ["a"]);

        for (bad, needle) in [
            ("doc: nope\nrequirement: x\n", "unknown"),
            (
                "doc: spec\nrequirement: x\ncontext:\n  ../x.md: y\n",
                "context",
            ),
            (
                "doc: spec\nrequirement: x\nexpect:\n  must_include: [\"(\"]\n",
                "正则",
            ),
            ("doc: spec\n", "requirement"),
        ] {
            let d = tempfile::TempDir::new().unwrap();
            std::fs::write(d.path().join("t.yaml"), bad).unwrap();
            let e = load_tasks(d.path()).unwrap_err();
            assert!(e.contains(needle), "{bad}: {e}");
        }
        let empty = tempfile::TempDir::new().unwrap();
        assert!(load_tasks(empty.path()).unwrap_err().contains("没有任务"));
        assert!(load_tasks(&empty.path().join("missing")).is_err());
    }
}
