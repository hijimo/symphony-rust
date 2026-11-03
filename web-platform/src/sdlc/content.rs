//! Document content rules: frontmatter (§4.4), body hash used for approval
//! binding, template validation (§5.1) and `## Files` parsing (§7).

use sha2::{Digest, Sha256};

use super::model::DocType;

/// Frontmatter written once when a document is created (§4.4).
pub fn frontmatter(doc: DocType, issue_iid: i64, template_ref: &str, base_sha: &str) -> String {
    format!(
        "---\nsymphony:\n  doc: {doc}\n  issue: {issue_iid}\n  template: {template_ref}\n  generated_by: {{ org_config_sha: \"\", base_sha: \"{base_sha}\" }}\n---\n"
    )
}

/// Strip a leading YAML frontmatter block (`---\n ... \n---\n`).
pub fn strip_frontmatter(content: &str) -> &str {
    let normalized = content.strip_prefix('\u{feff}').unwrap_or(content);
    let Some(rest) = normalized.strip_prefix("---\n") else {
        return normalized;
    };
    match rest.find("\n---\n") {
        Some(end) => &rest[end + 5..],
        None => match rest.strip_suffix("\n---") {
            Some(_) => "",
            None => normalized,
        },
    }
}

/// The leading frontmatter block (including delimiters), if any.
pub fn frontmatter_block(content: &str) -> Option<&str> {
    let body = strip_frontmatter(content);
    let fm_len = content.len() - body.len();
    (fm_len > 0).then(|| &content[..fm_len])
}

/// Keep the original frontmatter (written once at creation, §4.4): replace
/// whatever frontmatter `new` has with `original_fm`.
pub fn restore_frontmatter(original_fm: &str, new: &str) -> String {
    format!("{original_fm}{}", strip_frontmatter(new))
}

/// Patterns that indicate credentials leaked into a document (verify A I-1).
const SECRET_MARKERS: &[&str] = &[
    "-----BEGIN ",
    "\"refresh_token\"",
    "\"access_token\"",
    "\"id_token\"",
    "\"OPENAI_API_KEY\"",
    "glpat-",
    "ghp_",
    "github_pat_",
    "gho_",
    "xoxb-",
    "AKIA",
];

/// Return the first secret-like marker found in `text`, if any. `extra`
/// holds high-entropy values that must never appear (e.g. the contents of
/// the codex credential file).
pub fn find_secret(text: &str, extra: &[String]) -> Option<String> {
    for m in SECRET_MARKERS {
        if text.contains(m) {
            return Some(m.trim_matches('"').to_string());
        }
    }
    for v in extra {
        if v.len() >= 16 && text.contains(v.as_str()) {
            return Some("凭据内容".to_string());
        }
    }
    None
}

/// Body hash used for approval binding: sha256 of the body without
/// frontmatter, with line endings normalized and trailing whitespace trimmed,
/// so local edits to frontmatter never void an approval (§4.4).
pub fn body_hash(content: &str) -> String {
    let body = strip_frontmatter(content).replace("\r\n", "\n");
    let body = body.trim_end();
    hex::encode(Sha256::digest(body.as_bytes()))
}

/// Version used for optimistic locking of raw file content (workbench §4.1).
pub fn content_version(content: &[u8]) -> String {
    hex::encode(Sha256::digest(content))
}

/// Level-2 headings (`## Title`) and their bodies, in order.
pub fn sections(content: &str) -> Vec<(String, String)> {
    let body = strip_frontmatter(content);
    let mut out: Vec<(String, String)> = Vec::new();
    let mut in_fence = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if !in_fence {
            if let Some(title) = line.strip_prefix("## ") {
                out.push((title.trim().to_string(), String::new()));
                continue;
            }
        }
        if let Some((_, text)) = out.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    out
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn has_content(text: &str) -> bool {
    strip_comments(text).lines().any(|l| !l.trim().is_empty())
}

/// One validation failure, anchored to a section for the workbench (§5.4).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ValidationIssue {
    pub section: String,
    pub message: String,
}

/// Required sections per document type (built-in templates).
pub fn required_sections(doc: DocType) -> &'static [&'static str] {
    match doc {
        DocType::Intent => &[
            "Problem",
            "Proposed outcome",
            "Affected users and systems",
            "Constraints",
            "Open questions",
        ],
        DocType::Spec => &["Requirements", "Design", "Concerns"],
        DocType::Plan => &[
            "Files",
            "Steps",
            "Tests",
            "Risks",
            "Alternatives considered",
        ],
    }
}

/// Validate a document before it is submitted for review (§5.1).
///
/// `readonly_aliases` are multi-repo aliases that are not writable in the
/// implementation phase; listing their files in `## Files` is a warning.
pub fn validate(doc: DocType, content: &str, readonly_aliases: &[String]) -> Vec<ValidationIssue> {
    let secs = sections(content);
    let mut issues = Vec::new();
    for req in required_sections(doc) {
        match secs.iter().find(|(t, _)| t.eq_ignore_ascii_case(req)) {
            None => issues.push(ValidationIssue {
                section: req.to_string(),
                message: format!("缺少必填章节 ## {req}"),
            }),
            Some((_, body)) if !has_content(body) => issues.push(ValidationIssue {
                section: req.to_string(),
                message: format!("章节 ## {req} 不能为空"),
            }),
            _ => {}
        }
    }
    match doc {
        DocType::Spec => {
            if let Some((_, body)) = secs
                .iter()
                .find(|(t, _)| t.eq_ignore_ascii_case("Concerns"))
            {
                for item in list_items(body) {
                    let lower = item.to_lowercase();
                    let has_owner = lower.contains("owner:") || item.contains("负责人");
                    let has_resolution = lower.contains("resolution:") || item.contains("结论");
                    if !(has_owner && has_resolution) {
                        issues.push(ValidationIssue {
                            section: "Concerns".to_string(),
                            message: format!("关注点缺少负责人或结论：{item}"),
                        });
                    }
                }
            }
        }
        DocType::Plan => {
            let files = plan_files(content);
            if secs.iter().any(|(t, _)| t.eq_ignore_ascii_case("Files")) && files.is_empty() {
                issues.push(ValidationIssue {
                    section: "Files".to_string(),
                    message: "## Files 至少列出一个文件路径（每行一个，建议以 - 开头，例如 - src/app.ts）".to_string(),
                });
            }
            for f in &files {
                if f.starts_with('/') || f.split('/').any(|seg| seg == "..") {
                    issues.push(ValidationIssue {
                        section: "Files".to_string(),
                        message: format!("路径必须是工作区相对路径：{f}"),
                    });
                }
                if let Some(alias) = f.split('/').next() {
                    if readonly_aliases.iter().any(|a| a == alias) {
                        issues.push(ValidationIssue {
                            section: "Files".to_string(),
                            message: format!("仓库 {alias} 在实现阶段不可写：{f}"),
                        });
                    }
                }
            }
        }
        DocType::Intent => {}
    }
    issues
}

fn list_items(body: &str) -> Vec<String> {
    strip_comments(body)
        .lines()
        .filter_map(|l| {
            let t = l.trim_start();
            t.strip_prefix("- ")
                .or_else(|| t.strip_prefix("* "))
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Lines of the `## Files` section: `- ` / `* ` bullets, or bare lines that
/// look like a path (contain `/` or a file extension). AI drafts often drop
/// the bullet marker; such lines are still accepted.
fn files_lines(body: &str) -> Vec<String> {
    strip_comments(body)
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            if let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
                return Some(item.trim().to_string());
            }
            let first = t.trim_start_matches('`').split(['`', ' ', '\t']).next()?;
            let looks_like_path = !first.is_empty()
                && !first.starts_with('#')
                && (first.contains('/')
                    || first
                        .rsplit_once('.')
                        .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty()));
            looks_like_path.then(|| t.to_string())
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Paths listed in the plan's `## Files` section (one per line, `- ` bullet
/// preferred; an optional backtick-quoted path and trailing description are
/// allowed).
pub fn plan_files(content: &str) -> Vec<String> {
    let secs = sections(content);
    let Some((_, body)) = secs.iter().find(|(t, _)| t.eq_ignore_ascii_case("Files")) else {
        return Vec::new();
    };
    files_lines(body)
        .into_iter()
        .filter_map(|item| {
            let path = if let Some(rest) = item.strip_prefix('`') {
                rest.split('`').next().unwrap_or_default().to_string()
            } else {
                item.split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_string()
            };
            let path = path.trim_end_matches([':', ',', '；', '：']).to_string();
            (!path.is_empty()).then_some(path)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "---\nsymphony:\n  doc: plan\n---\n# Plan\n\n## Files\n- `frontend/src/pages/New.tsx` new page\n- backend/src/api.rs\n\n## Steps\n1. a\n\n## Tests\n- t\n\n## Risks\n- r\n\n## Alternatives considered\n- none\n";

    #[test]
    fn frontmatter_is_stripped_and_hash_ignores_it() {
        let a = format!(
            "{}# T\nbody\n",
            frontmatter(DocType::Spec, 1, "spec@1", "abc")
        );
        let b = format!(
            "{}# T\nbody\n",
            frontmatter(DocType::Spec, 1, "spec@9", "zzz")
        );
        assert_eq!(strip_frontmatter(&a), "# T\nbody\n");
        assert_eq!(body_hash(&a), body_hash(&b));
        assert_ne!(body_hash(&a), body_hash("# T\nother\n"));
        // CRLF and trailing whitespace are normalized.
        assert_eq!(body_hash("# T\r\nbody\r\n\n"), body_hash("# T\nbody"));
        assert_eq!(strip_frontmatter("no fm"), "no fm");
    }

    #[test]
    fn secret_detection() {
        assert!(find_secret("token glpat-abc", &[]).is_some());
        assert!(find_secret("{\"refresh_token\": \"x\"}", &[]).is_some());
        assert!(find_secret("plain text", &[]).is_none());
        let v = "0123456789abcdefSECRET".to_string();
        assert!(find_secret(&format!("x {v} y"), std::slice::from_ref(&v)).is_some());
        assert!(find_secret("short", &["short".to_string()]).is_none());
    }

    #[test]
    fn frontmatter_restore() {
        let fm = frontmatter(DocType::Plan, 3, "plan@1", "b");
        let orig = format!("{fm}# A\n");
        assert_eq!(frontmatter_block(&orig), Some(fm.as_str()));
        assert_eq!(restore_frontmatter(&fm, "# B\n"), format!("{fm}# B\n"));
        assert_eq!(
            restore_frontmatter(&fm, "---\nx: 1\n---\n# C\n"),
            format!("{fm}# C\n")
        );
        assert_eq!(frontmatter_block("# none"), None);
    }

    #[test]
    fn sections_skip_fenced_code() {
        let s = sections("## A\nx\n```\n## not\n```\n## B\ny\n");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].0, "A");
        assert!(s[0].1.contains("## not"));
    }

    #[test]
    fn validate_plan_ok_and_files() {
        assert!(validate(DocType::Plan, PLAN, &[]).is_empty());
        assert_eq!(
            plan_files(PLAN),
            vec!["frontend/src/pages/New.tsx", "backend/src/api.rs"]
        );
        let issues = validate(DocType::Plan, PLAN, &["backend".to_string()]);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("不可写"));
    }

    #[test]
    fn plan_files_accepts_bare_path_lines() {
        let plan = PLAN.replace(
            "- `frontend/src/pages/New.tsx` new page\n- backend/src/api.rs\n",
            "<!-- hint -->\nprimary/src/a.tsx\n`jxyai/Jxy.Model/B.cs` 新增\nREADME.md\n",
        );
        assert!(validate(DocType::Plan, &plan, &[]).is_empty());
        assert_eq!(
            plan_files(&plan),
            vec!["primary/src/a.tsx", "jxyai/Jxy.Model/B.cs", "README.md"]
        );
    }

    #[test]
    fn plan_files_ignores_prose_and_comments() {
        let plan = PLAN.replace(
            "- `frontend/src/pages/New.tsx` new page\n- backend/src/api.rs\n",
            "<!-- - src/hint.rs -->\n暂未确定要改哪些文件\n",
        );
        assert!(plan_files(&plan).is_empty());
        assert!(validate(DocType::Plan, &plan, &[])
            .iter()
            .any(|i| i.section == "Files"));
    }

    #[test]
    fn validate_missing_and_empty_sections() {
        let issues = validate(DocType::Intent, "## Problem\n<!-- hint -->\n", &[]);
        assert!(issues.iter().any(|i| i.message.contains("不能为空")));
        assert!(issues.iter().any(|i| i.section == "Constraints"));
    }

    #[test]
    fn validate_plan_rejects_bad_paths_and_empty_files() {
        let bad = PLAN.replace("backend/src/api.rs", "../etc/passwd");
        assert!(validate(DocType::Plan, &bad, &[])
            .iter()
            .any(|i| i.message.contains("相对路径")));
        let empty = PLAN.replace(
            "- `frontend/src/pages/New.tsx` new page\n- backend/src/api.rs\n",
            "text only\n",
        );
        assert!(validate(DocType::Plan, &empty, &[])
            .iter()
            .any(|i| i.section == "Files"));
    }

    #[test]
    fn validate_spec_concerns() {
        let ok = "## Requirements\nr\n## Design\nd\n## Concerns\n- PII. owner: sec. resolution: masked\n";
        assert!(validate(DocType::Spec, ok, &[]).is_empty());
        let bad = "## Requirements\nr\n## Design\nd\n## Concerns\n- PII leak\n";
        assert_eq!(validate(DocType::Spec, bad, &[]).len(), 1);
        let none = "## Requirements\nr\n## Design\nd\n## Concerns\n无\n";
        assert!(validate(DocType::Spec, none, &[]).is_empty());
    }

    #[test]
    fn content_version_is_stable() {
        assert_eq!(content_version(b"x"), content_version(b"x"));
        assert_ne!(content_version(b"x"), content_version(b"y"));
    }
}
