//! Self-verification support (self-verification design §3).
//!
//! Pure helpers used by the runner after every run:
//! - blocked markers written by the agent (`<!-- symphony:blocked ... -->`),
//! - the validation report table (`<!-- symphony:validation ... -->`),
//! - change-type classification by path rules and the required baseline steps,
//! - the per-run E2E env file parsed from the issue description.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Length used for every SHA written into markers and reports.
pub const SHORT_SHA_LEN: usize = 12;

pub const LABEL_VALIDATION_BLOCKED: &str = "validation-blocked";
pub const LABEL_VALIDATION_MISSING: &str = "validation-missing";
pub const LABEL_BLOCKED_ACCESS: &str = "blocked-access";
pub const ALL_LABELS: [&str; 3] = [
    LABEL_VALIDATION_BLOCKED,
    LABEL_VALIDATION_MISSING,
    LABEL_BLOCKED_ACCESS,
];

const BLOCKED_PREFIX: &str = "<!-- symphony:blocked ";
const BLOCKED_CLEARED_PREFIX: &str = "<!-- symphony:blocked-cleared ";
const VALIDATION_PREFIX: &str = "<!-- symphony:validation";

// ---------------------------------------------------------------- markers

/// Parsed `<!-- symphony:blocked run=... reason=... heads=a:sha,b:sha -->`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedMarker {
    pub run: String,
    pub reason: String,
    pub heads: BTreeMap<String, String>,
}

impl BlockedMarker {
    /// Label to add for this marker's reason.
    pub fn label(&self) -> &'static str {
        if self.reason == "access" {
            LABEL_BLOCKED_ACCESS
        } else {
            LABEL_VALIDATION_BLOCKED
        }
    }
}

fn parse_kv(body: &str) -> BTreeMap<String, String> {
    body.split_whitespace()
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn parse_heads(value: &str) -> BTreeMap<String, String> {
    value
        .split(',')
        .filter_map(|p| p.split_once(':'))
        .map(|(a, s)| (a.trim().to_string(), s.trim().to_lowercase()))
        .filter(|(a, s)| !a.is_empty() && !s.is_empty())
        .collect()
}

/// All active (not cleared) blocked markers in `text`, in order.
pub fn blocked_markers(text: &str) -> Vec<BlockedMarker> {
    let mut out = Vec::new();
    for (idx, _) in text.match_indices(BLOCKED_PREFIX) {
        let rest = &text[idx + BLOCKED_PREFIX.len()..];
        let Some(end) = rest.find("-->") else {
            continue;
        };
        let kv = parse_kv(&rest[..end]);
        let (Some(run), Some(reason)) = (kv.get("run"), kv.get("reason")) else {
            continue;
        };
        out.push(BlockedMarker {
            run: run.clone(),
            reason: reason.clone(),
            heads: kv.get("heads").map(|h| parse_heads(h)).unwrap_or_default(),
        });
    }
    out
}

/// Rewrite every active blocked marker into a cleared one. Returns `None`
/// when nothing changed.
pub fn clear_blocked_markers(text: &str) -> Option<String> {
    if !text.contains(BLOCKED_PREFIX) {
        return None;
    }
    Some(text.replace(BLOCKED_PREFIX, BLOCKED_CLEARED_PREFIX))
}

/// SHA prefixes match when both have at least 7 chars and one is a prefix
/// of the other (case-insensitive).
pub fn sha_matches(a: &str, b: &str) -> bool {
    let a = a.trim().to_lowercase();
    let b = b.trim().to_lowercase();
    a.len() >= 7 && b.len() >= 7 && (a.starts_with(&b) || b.starts_with(&a))
}

fn heads_match(marker: &BTreeMap<String, String>, current: &BTreeMap<String, String>) -> bool {
    !current.is_empty()
        && current.len() == marker.len()
        && current
            .iter()
            .all(|(alias, sha)| marker.get(alias).is_some_and(|m| sha_matches(m, sha)))
}

/// The marker that is effective for this run: written by `run_id` and its
/// heads equal the current HEAD of every writable repository.
pub fn effective_blocked_marker<'a>(
    markers: &'a [BlockedMarker],
    run_id: &str,
    current_heads: &BTreeMap<String, String>,
) -> Option<&'a BlockedMarker> {
    markers
        .iter()
        .rev()
        .find(|m| m.run == run_id && heads_match(&m.heads, current_heads))
}

// ------------------------------------------------------- validation report

/// Result value of one validation row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepResult {
    Passed,
    Failed,
    Unverified,
    Skipped,
}

impl StepResult {
    fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "通过" | "passed" | "pass" => Some(Self::Passed),
            "失败" | "failed" | "fail" => Some(Self::Failed),
            "未验证" | "unverified" => Some(Self::Unverified),
            "跳过" | "skipped" | "skip" => Some(Self::Skipped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationRow {
    pub repo: String,
    pub head: String,
    pub step: String,
    pub result: StepResult,
}

/// The last `symphony:validation` block: `(round, rows)`.
pub fn last_validation_block(text: &str) -> Option<(u32, Vec<ValidationRow>)> {
    let idx = text.rfind(VALIDATION_PREFIX)?;
    let after = &text[idx..];
    let header_end = after.find("-->")?;
    let kv = parse_kv(&after[VALIDATION_PREFIX.len()..header_end]);
    let round = kv.get("round").and_then(|r| r.parse().ok()).unwrap_or(0);
    let mut rows = Vec::new();
    let mut started = false;
    for line in after[header_end + 3..].lines() {
        let t = line.trim();
        if !t.starts_with('|') {
            if started {
                break;
            }
            if t.is_empty() || t.starts_with("```") {
                continue;
            }
            break;
        }
        started = true;
        let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();
        // | 仓库 | head | 类型 | 步骤 | 命令（来源） | 结果 | 备注 |
        if cells.len() < 6 || cells[0] == "仓库" || cells[0].starts_with("---") {
            continue;
        }
        if let Some(result) = StepResult::parse(cells[5]) {
            rows.push(ValidationRow {
                repo: cells[0].to_string(),
                head: cells[1].to_lowercase(),
                step: cells[3].to_uppercase(),
                result,
            });
        }
    }
    Some((round, rows))
}

// ------------------------------------------------ change-type classification

/// Technology stacks detected from marker files at the repository root.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stack {
    pub frontend: bool,
    pub backend: bool,
}

pub fn detect_stack(repo_dir: &Path) -> Stack {
    let frontend = repo_dir.join("package.json").is_file();
    let backend = std::fs::read_dir(repo_dir)
        .map(|it| {
            it.flatten().any(|e| {
                let n = e.file_name().to_string_lossy().to_lowercase();
                n.ends_with(".sln") || n.ends_with(".slnx") || n.ends_with(".csproj")
            })
        })
        .unwrap_or(false);
    Stack { frontend, backend }
}

/// Change type of one file (design §3.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeType {
    Docs,
    BuildConfig,
    Test,
    FrontendStyle,
    FrontendLogic,
    BackendLogic,
    Database,
}

fn ext(path: &str) -> String {
    path.rsplit('/')
        .next()
        .and_then(|f| f.rsplit_once('.'))
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default()
}

fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_lowercase()
}

/// Classify one repo-relative path. `stack` decides what "source" means.
pub fn classify(path: &str, stack: Stack) -> ChangeType {
    let p = path.replace('\\', "/");
    let lower = p.to_lowercase();
    let name = file_name(&p);
    let e = ext(&p);
    let segs: Vec<&str> = lower.split('/').collect();

    if e == "sql" || segs.iter().any(|s| *s == "migrations" || *s == "migration") {
        return ChangeType::Database;
    }
    let is_test = segs.contains(&"__tests__")
        || segs.first() == Some(&"e2e")
        || segs
            .iter()
            .any(|s| s.ends_with(".test") || s.ends_with(".tests"))
        || name.contains(".test.")
        || name.contains(".spec.");
    if is_test {
        return ChangeType::Test;
    }
    if e == "md" || segs.first() == Some(&"docs") {
        return ChangeType::Docs;
    }
    let build_files = [
        "package.json",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "tsconfig.json",
        ".gitlab-ci.yml",
        "global.json",
        "nuget.config",
    ];
    let is_build = build_files.contains(&name.as_str())
        || name.starts_with("tsconfig")
        || name.starts_with("vite.config")
        || name.starts_with("eslint")
        || name.starts_with(".eslintrc")
        || name.starts_with("appsettings")
        || name.starts_with("directory.")
        || matches!(e.as_str(), "csproj" | "sln" | "slnx" | "props" | "targets")
        || segs.contains(&"settings")
        || segs.first() == Some(&".github");
    if is_build {
        return ChangeType::BuildConfig;
    }
    if matches!(e.as_str(), "css" | "less" | "scss" | "sass") {
        return ChangeType::FrontendStyle;
    }
    match (stack.frontend, stack.backend) {
        (_, true) if matches!(e.as_str(), "cs" | "fs" | "vb") => ChangeType::BackendLogic,
        (true, _) => ChangeType::FrontendLogic,
        (false, true) => ChangeType::BackendLogic,
        (false, false) => ChangeType::FrontendLogic,
    }
}

/// Baseline steps that must pass for a set of changed files (design §3.1,
/// §3.2.2). Returns `None` when the stack cannot be identified for a change
/// that needs one (the check then reports "unverified").
pub fn required_baseline(changed: &[String], stack: Stack) -> Option<BTreeSet<&'static str>> {
    let mut need = BTreeSet::new();
    for f in changed {
        let t = classify(f, stack);
        let fe = |n: &mut BTreeSet<&'static str>| {
            n.extend(["F1", "F2", "F3"]);
        };
        let be = |n: &mut BTreeSet<&'static str>| {
            n.extend(["B1", "B5"]);
        };
        match t {
            ChangeType::Docs => {}
            ChangeType::FrontendStyle | ChangeType::FrontendLogic => fe(&mut need),
            ChangeType::BackendLogic | ChangeType::Database => be(&mut need),
            ChangeType::BuildConfig | ChangeType::Test => {
                if !stack.frontend && !stack.backend {
                    return None;
                }
                if stack.frontend {
                    if t == ChangeType::Test {
                        need.extend(["F1", "F3"]);
                    } else {
                        fe(&mut need);
                    }
                }
                if stack.backend {
                    be(&mut need);
                }
            }
        }
    }
    if !need.is_empty() && !stack.frontend && !stack.backend {
        return None;
    }
    Some(need)
}

// ---------------------------------------------------------------- E2E env

/// Parse the `## E2E` section of an issue description into environment
/// variables. Returns `(vars, problems)`.
///
/// Accepted lines: `- 环境变量: K=V; K2=V2` and `- 后端: 本地|开发环境`.
/// Any other key (for example a frontend address) is rejected: connection
/// targets never come from the issue (design §3.3).
pub fn parse_e2e_section(description: &str) -> (BTreeMap<String, String>, Vec<String>) {
    let mut vars = BTreeMap::new();
    let mut problems = Vec::new();
    let mut in_section = false;
    for line in description.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            in_section = t.trim_start_matches('#').trim().eq_ignore_ascii_case("e2e");
            continue;
        }
        if !in_section || t.is_empty() {
            continue;
        }
        let item = t.trim_start_matches(['-', '*']).trim();
        let Some((key, value)) = item.split_once([':', '：']) else {
            continue;
        };
        let value = value.split(" #").next().unwrap_or(value).trim();
        match key.trim() {
            "环境变量" | "env" => {
                for pair in value.split(';') {
                    let Some((k, v)) = pair.split_once('=') else {
                        continue;
                    };
                    let k = k.trim();
                    if !k.is_empty()
                        && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && !k.starts_with(|c: char| c.is_ascii_digit())
                    {
                        vars.insert(k.to_string(), v.trim().to_string());
                    } else {
                        problems.push(format!("invalid variable name `{k}`"));
                    }
                }
            }
            "后端" | "backend" => match value {
                "本地" | "local" => {
                    vars.insert("SYMPHONY_E2E_BACKEND".into(), "local".into());
                }
                "开发环境" | "dev" => {
                    vars.insert("SYMPHONY_E2E_BACKEND".into(), "dev".into());
                }
                other => problems.push(format!("unsupported backend `{other}`")),
            },
            other => problems.push(format!("unsupported E2E field `{other}`")),
        }
    }
    (vars, problems)
}

fn shell_quote(v: &str) -> String {
    format!("'{}'", v.replace('\'', "'\\''"))
}

/// Directory holding per-run E2E files, outside every issue workspace.
pub fn e2e_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".symphony").join("e2e")
}

/// Write `<root>/.symphony/e2e/<run_id>/e2e.env` (dir 0700, file 0600).
/// Returns the file path, or `None` when the issue has no usable E2E data.
pub fn write_e2e_env(
    workspace_root: &Path,
    run_id: &str,
    description: Option<&str>,
) -> std::io::Result<Option<(PathBuf, Vec<String>)>> {
    let Some(desc) = description else {
        return Ok(None);
    };
    let (vars, problems) = parse_e2e_section(desc);
    if !problems.is_empty() || vars.is_empty() {
        return Ok(if problems.is_empty() {
            None
        } else {
            // Reject the whole section: never hand over a partial target.
            Some((PathBuf::new(), problems))
        });
    }
    let dir = e2e_root(workspace_root).join(run_id);
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let file = dir.join("e2e.env");
    let mut body = String::new();
    for (k, v) in &vars {
        body.push_str(&format!("{k}={}\n", shell_quote(v)));
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&file)?;
        f.write_all(body.as_bytes())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&file, body)?;
    Ok(Some((file, Vec::new())))
}

/// Remove E2E run directories that do not belong to a running run.
pub fn cleanup_e2e(workspace_root: &Path, keep: &[&str]) {
    let root = e2e_root(workspace_root);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !keep.contains(&name.as_str()) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heads(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(a, s)| (a.to_string(), s.to_string()))
            .collect()
    }

    #[test]
    fn blocked_marker_effective_only_for_same_run_and_heads() {
        let wp = "## Codex Workpad\n<!-- symphony:blocked run=r1 reason=validation heads=primary:0802a8c1d2e3 -->\nold\n\
                  <!-- symphony:blocked run=r2 reason=access heads=primary:0802a8c1d2e3 -->\n";
        let ms = blocked_markers(wp);
        assert_eq!(ms.len(), 2);
        let cur = heads(&[("primary", "0802a8c1d2e3ffff")]);
        // Previous run's marker never counts.
        assert!(effective_blocked_marker(&ms, "r3", &cur).is_none());
        let m = effective_blocked_marker(&ms, "r2", &cur).unwrap();
        assert_eq!(m.label(), LABEL_BLOCKED_ACCESS);
        assert_eq!(
            effective_blocked_marker(&ms, "r1", &cur).unwrap().label(),
            LABEL_VALIDATION_BLOCKED
        );
        // HEAD moved after the marker: not effective.
        let moved = heads(&[("primary", "ffffffffffff")]);
        assert!(effective_blocked_marker(&ms, "r2", &moved).is_none());
    }

    #[test]
    fn multi_repo_heads_must_all_match() {
        let wp = "<!-- symphony:blocked run=r reason=validation heads=primary:aaaaaaaaaaaa,jxyai:bbbbbbbbbbbb -->";
        let ms = blocked_markers(wp);
        let ok = heads(&[("primary", "aaaaaaaaaaaa"), ("jxyai", "bbbbbbbbbbbb")]);
        assert!(effective_blocked_marker(&ms, "r", &ok).is_some());
        let partial = heads(&[("primary", "aaaaaaaaaaaa")]);
        assert!(effective_blocked_marker(&ms, "r", &partial).is_none());
        let wrong = heads(&[("primary", "aaaaaaaaaaaa"), ("jxyai", "cccccccccccc")]);
        assert!(effective_blocked_marker(&ms, "r", &wrong).is_none());
    }

    #[test]
    fn clearing_rewrites_markers_and_same_text_rewritten_is_still_new_run() {
        let wp = "<!-- symphony:blocked run=r1 reason=validation heads=primary:aaaaaaaaaaaa -->";
        let cleared = clear_blocked_markers(wp).unwrap();
        assert!(blocked_markers(&cleared).is_empty());
        assert!(cleared.contains("symphony:blocked-cleared run=r1"));
        assert!(clear_blocked_markers(&cleared).is_none());
        // Todo-continue: the new run writes an identical marker except run id.
        let again = format!("{cleared}\n<!-- symphony:blocked run=r2 reason=validation heads=primary:aaaaaaaaaaaa -->");
        let ms = blocked_markers(&again);
        assert!(
            effective_blocked_marker(&ms, "r2", &heads(&[("primary", "aaaaaaaaaaaa")])).is_some()
        );
    }

    #[test]
    fn sha_prefix_rules() {
        assert!(sha_matches("0802a8c", "0802a8c1d2e3"));
        assert!(sha_matches("0802A8C1D2E3", "0802a8c1d2e3"));
        assert!(!sha_matches("0802a8", "0802a8c1d2e3"));
        assert!(!sha_matches("0802a8d", "0802a8c1d2e3"));
    }

    #[test]
    fn parses_last_validation_block() {
        let wp = "\
<!-- symphony:validation round=1 -->
| 仓库 | head | 类型 | 步骤 | 命令（来源） | 结果 | 备注 |
|---|---|---|---|---|---|---|
| primary | 111111111111 | 前端逻辑 | F1 | pnpm lint | 失败 | |

<!-- symphony:validation round=2 -->
| 仓库 | head | 类型 | 步骤 | 命令（来源） | 结果 | 备注 |
|---|---|---|---|---|---|---|
| primary | 0802a8c1d2e3 | 前端逻辑 | F1 | pnpm lint | 通过 | |
| primary | 0802a8c1d2e3 | 前端逻辑 | f5 | — | 跳过 | no e2e |
trailing text
";
        let (round, rows) = last_validation_block(wp).unwrap();
        assert_eq!(round, 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].step, "F1");
        assert_eq!(rows[0].result, StepResult::Passed);
        assert_eq!(rows[1].step, "F5");
        assert_eq!(rows[1].result, StepResult::Skipped);
        assert!(last_validation_block("nothing").is_none());
    }

    #[test]
    fn classification_rules() {
        let fe = Stack {
            frontend: true,
            backend: false,
        };
        let be = Stack {
            frontend: false,
            backend: true,
        };
        assert_eq!(classify("docs/a.md", fe), ChangeType::Docs);
        assert_eq!(classify("README.md", fe), ChangeType::Docs);
        assert_eq!(classify("docs/20261030-x.sql", be), ChangeType::Database);
        assert_eq!(classify("src/a/__tests__/x.test.tsx", fe), ChangeType::Test);
        assert_eq!(classify("e2e/flow.spec.ts", fe), ChangeType::Test);
        assert_eq!(classify("Jxy.Test/A/BTest.cs", be), ChangeType::Test);
        assert_eq!(classify("Jxy.Ai.Tests/A.cs", be), ChangeType::Test);
        assert_eq!(classify("pnpm-lock.yaml", fe), ChangeType::BuildConfig);
        assert_eq!(
            classify("Jxy.Model/Jxy.Model.csproj", be),
            ChangeType::BuildConfig
        );
        assert_eq!(
            classify("Jxy.Setting/settings/Local/db.json", be),
            ChangeType::BuildConfig
        );
        assert_eq!(classify("src/index.less", fe), ChangeType::FrontendStyle);
        assert_eq!(classify("src/pages/x.tsx", fe), ChangeType::FrontendLogic);
        assert_eq!(classify("Jxy.Business/A.cs", be), ChangeType::BackendLogic);
    }

    #[test]
    fn baselines() {
        let fe = Stack {
            frontend: true,
            backend: false,
        };
        let be = Stack {
            frontend: false,
            backend: true,
        };
        let none = Stack::default();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(required_baseline(&s(&["docs/a.md"]), fe)
            .unwrap()
            .is_empty());
        assert_eq!(
            required_baseline(&s(&["src/x.tsx"]), fe).unwrap(),
            ["F1", "F2", "F3"].into_iter().collect()
        );
        assert_eq!(
            required_baseline(&s(&["src/a/__tests__/x.test.ts"]), fe).unwrap(),
            ["F1", "F3"].into_iter().collect()
        );
        assert_eq!(
            required_baseline(&s(&["A/B.cs", "docs/x.sql"]), be).unwrap(),
            ["B1", "B5"].into_iter().collect()
        );
        // Unknown stack for code changes: cannot decide.
        assert!(required_baseline(&s(&["src/x.rs"]), none).is_none());
        assert!(required_baseline(&s(&["Makefile.json", "package.json"]), none).is_none());
        // Docs only with unknown stack: nothing required.
        assert!(required_baseline(&s(&["docs/a.md"]), none)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn e2e_section_parsing_and_rejections() {
        let d = "需求...\n\n## E2E\n- 环境变量: E2E_TOKEN=abc; E2E_USER_INFO={\"id\":1}\n- 后端: 本地\n\n## Other\n- 前端地址: http://evil";
        let (vars, problems) = parse_e2e_section(d);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(vars["E2E_TOKEN"], "abc");
        assert_eq!(vars["E2E_USER_INFO"], "{\"id\":1}");
        assert_eq!(vars["SYMPHONY_E2E_BACKEND"], "local");

        let bad = "## E2E\n- 环境变量: E2E_TOKEN=abc\n- 前端地址: https://prod.example.com\n";
        let (_, problems) = parse_e2e_section(bad);
        assert_eq!(problems.len(), 1);
        let bad2 = "## E2E\n- 后端: https://prod\n- 环境变量: 1X=1; A-B=2\n";
        assert_eq!(parse_e2e_section(bad2).1.len(), 3);
    }

    #[test]
    fn e2e_env_file_written_private_and_cleaned() {
        let tmp = tempfile::tempdir().unwrap();
        let d = "## E2E\n- 环境变量: E2E_TOKEN=it's secret\n";
        let (path, problems) = write_e2e_env(tmp.path(), "run-1", Some(d))
            .unwrap()
            .unwrap();
        assert!(problems.is_empty());
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body, "E2E_TOKEN='it'\\''s secret'\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            let dmode = std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(dmode, 0o700);
        }
        // Rejected section: no file at all.
        let (p, probs) = write_e2e_env(tmp.path(), "run-2", Some("## E2E\n- 前端地址: x\n"))
            .unwrap()
            .unwrap();
        assert!(p.as_os_str().is_empty());
        assert_eq!(probs.len(), 1);
        assert!(!e2e_root(tmp.path()).join("run-2").exists());
        assert!(write_e2e_env(tmp.path(), "run-3", Some("no e2e"))
            .unwrap()
            .is_none());

        std::fs::create_dir_all(e2e_root(tmp.path()).join("stale")).unwrap();
        cleanup_e2e(tmp.path(), &["run-1"]);
        assert!(path.exists());
        assert!(!e2e_root(tmp.path()).join("stale").exists());
        cleanup_e2e(tmp.path(), &[]);
        assert!(!path.exists());
    }
}
