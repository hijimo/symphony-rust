//! Document templates (§5): built-in defaults based on the Playbook, optionally
//! overridden by `docs/changes/_templates/<doc>.md` in the primary repository.
//! Organisation-level templates (codex-org-config) are not in phase 1.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::model::{DocType, TEMPLATES_DIR};

/// Built-in template version. Bump when the built-in skeleton changes.
pub const BUILTIN_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DocTemplate {
    pub doc_type: DocType,
    /// `builtin` or `project`.
    pub source: String,
    /// e.g. `spec@1` or `spec@project:<hash8>`.
    pub template_ref: String,
    pub skeleton: String,
    pub prompt: String,
    pub checklist: Vec<String>,
}

const INTENT_SKELETON: &str = "# Intent: <title>\n\n## Problem\n<!-- 用提出者自己的话：今天做不到什么、谁受影响。不需要正式语言 -->\n\n## Proposed outcome\n<!-- 更好的状态是什么样 -->\n\n## Affected users and systems\n<!-- 受影响的用户和系统 -->\n\n## Constraints\n<!-- 约束：安全、合规、性能、兼容性等 -->\n\n## Open questions\n<!-- 还没想清楚的问题 -->\n";

const SPEC_SKELETON: &str = "# Spec: <title>\n\n## Requirements\n<!-- 需求：要做什么、验收标准 -->\n\n## Design\n<!-- 设计：如何集成进现有代码库，接口、数据、交互 -->\n\n## Concerns\n<!-- 关注点：每条一行，写明 owner: <负责人> resolution: <结论>；没有则写“无” -->\n";

const PLAN_SKELETON: &str = "# Plan: <title>\n\n## Files\n<!-- 每行一个 Markdown 列表项，以 \"- \" 开头，写工作区相对路径，可在路径后加说明；多仓库项目以仓库别名开头。例如：\n- src/pages/Example.tsx 新增页面\n- backend/src/api.rs 增加接口\n-->\n\n## Steps\n<!-- 工作顺序 -->\n\n## Tests\n<!-- 证明改动有效的测试 -->\n\n## Risks\n<!-- 可能弄坏什么、风险最大的一步 -->\n\n## Alternatives considered\n<!-- 放弃了哪些方案、为什么 -->\n";

const INTENT_PROMPT: &str = "Help the proposer write intent.md in their own words: what they want, why, under which constraints, and what is still open. Ask the questions an analyst would ask (scope, users, constraints, what success looks like). Keep the section structure of the template.";

/// Playbook Design-stage prompt (verbatim), plus Symphony output rules.
const SPEC_PROMPT: &str = "Read the attached intent.md and produce a requirements and design spec for integrating it into our existing codebase. Apply the skills available to you so the plan conforms to our brand guidelines, security policies and UX standards. Document the spec fully as spec.md, ready to hand to the engineering team. Describe clearly any areas of concern, especially where you cannot satisfy contradicting policies.";

const PLAN_PROMPT: &str = "Read the approved intent.md (if any) and spec.md and produce an implementation plan as plan.md: which files to change (in ## Files, one Markdown bullet per file: a line starting with \"- \" followed by the workspace-relative path, e.g. \"- src/app.ts\"; for multi-repo projects start the path with the repository alias), the order of work, the tests that prove it works, the risks, and the alternatives you rejected. Iterate until an engineer who never saw this conversation could implement the change from the plan alone.";

fn builtin(doc: DocType) -> DocTemplate {
    let (skeleton, prompt, checklist): (&str, &str, &[&str]) = match doc {
        DocType::Intent => (
            INTENT_SKELETON,
            INTENT_PROMPT,
            &[
                "问题是否用提出者自己的话写清楚了？",
                "期望结果是否具体、可判断？",
                "约束和不在范围内的内容是否写明？",
            ],
        ),
        DocType::Spec => (
            SPEC_SKELETON,
            SPEC_PROMPT,
            &[
                "spec 是否解决了 intent 中陈述的问题？",
                "intent 的 Open questions 是已解决，还是被带到下一步？",
                "标出的关注点是否都有负责人和结论？",
            ],
        ),
        DocType::Plan => (
            PLAN_SKELETON,
            PLAN_PROMPT,
            &[
                "这个改动可能弄坏什么？",
                "哪一步风险最大？",
                "放弃了哪些方案、理由是否成立？",
                "plan 中是否有与需求无关的操作（外部网络访问、修改 CI、读取凭据等）？",
            ],
        ),
    };
    DocTemplate {
        doc_type: doc,
        source: "builtin".to_string(),
        template_ref: format!("{doc}@{BUILTIN_VERSION}"),
        skeleton: skeleton.to_string(),
        prompt: prompt.to_string(),
        checklist: checklist.iter().map(|s| s.to_string()).collect(),
    }
}

/// Optional front matter of a project template (P2 §5 A7).
#[derive(Debug, Default, Deserialize, Serialize)]
struct TemplateOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    checklist: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TemplateFrontMatter {
    symphony_template: TemplateOverrides,
}

/// `(yaml, body)` when the text starts with a `---` block.
fn split_front_matter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    Some((&rest[..end + 1], &rest[end + 5..]))
}

/// Project template from its file text. A leading front matter with a
/// `symphony_template` key overrides prompt and checklist; the rest is the
/// skeleton. Without that key the whole text is the skeleton.
pub fn from_project_text(doc: DocType, text: &str) -> Result<DocTemplate, String> {
    let mut tpl = builtin(doc);
    let hash = super::content::content_version(text.as_bytes());
    tpl.source = "project".to_string();
    tpl.template_ref = format!("{doc}@project:{}", &hash[..8]);
    tpl.skeleton = text.to_string();
    let Some((yaml, body)) = split_front_matter(text) else {
        return Ok(tpl);
    };
    let value: serde_yaml::Value =
        serde_yaml::from_str(yaml).map_err(|e| format!("front matter 不是合法的 YAML：{e}"))?;
    if value.get("symphony_template").is_none() {
        return Ok(tpl);
    }
    let fm: TemplateFrontMatter = serde_yaml::from_value(value)
        .map_err(|e| format!("front matter 的 symphony_template 格式不正确：{e}"))?;
    if let Some(p) = fm.symphony_template.prompt {
        tpl.prompt = p;
    }
    if let Some(c) = fm.symphony_template.checklist {
        tpl.checklist = c;
    }
    tpl.skeleton = body.to_string();
    Ok(tpl)
}

/// Problems that block saving a project template.
pub fn validate_template(doc: DocType, text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return vec!["模板不能为空".to_string()];
    }
    let tpl = match from_project_text(doc, text) {
        Ok(t) => t,
        Err(e) => return vec![e],
    };
    let mut out: Vec<String> = super::content::required_sections(doc)
        .iter()
        .filter(|r| !tpl.skeleton.contains(&format!("## {r}")))
        .map(|r| format!("骨架缺少必填章节 ## {r}"))
        .collect();
    if tpl.prompt.trim().is_empty() {
        out.push("prompt 不能为空".to_string());
    }
    out
}

/// Editable file text of a template: front matter with prompt and
/// checklist, then the skeleton.
pub fn editable_text(tpl: &DocTemplate) -> String {
    let fm = TemplateFrontMatter {
        symphony_template: TemplateOverrides {
            prompt: Some(tpl.prompt.clone()),
            checklist: Some(tpl.checklist.clone()),
        },
    };
    let yaml = serde_yaml::to_string(&fm).unwrap_or_default();
    format!("---\n{yaml}---\n{}", tpl.skeleton)
}

/// Resolve the effective template: project override when
/// `<repo_root>/docs/changes/_templates/<doc>.md` exists, else built-in.
pub fn resolve(doc: DocType, repo_root: Option<&Path>) -> DocTemplate {
    if let Some(root) = repo_root {
        let path = root.join(TEMPLATES_DIR).join(doc.file_name());
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Some(t) = resolve_text(doc, &text) {
                return t;
            }
        }
    }
    builtin(doc)
}

/// Project template from file text; `None` (built-in applies) when empty or
/// unparseable.
pub fn resolve_text(doc: DocType, text: &str) -> Option<DocTemplate> {
    if text.trim().is_empty() {
        return None;
    }
    match from_project_text(doc, text) {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!(%doc, "ignoring invalid project template: {e}");
            None
        }
    }
}

pub fn all(repo_root: Option<&Path>) -> Vec<DocTemplate> {
    DocType::ALL
        .iter()
        .map(|d| resolve(*d, repo_root))
        .collect()
}

/// Fill the `<title>` placeholder of a skeleton.
pub fn render_skeleton(tpl: &DocTemplate, title: &str) -> String {
    tpl.skeleton.replace("<title>", title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdlc::content;

    #[test]
    fn builtin_skeletons_contain_required_sections() {
        for d in DocType::ALL {
            let t = resolve(d, None);
            assert_eq!(t.source, "builtin");
            for req in content::required_sections(d) {
                assert!(
                    t.skeleton.contains(&format!("## {req}")),
                    "{d} missing {req}"
                );
            }
            assert!(!t.checklist.is_empty());
        }
        assert!(resolve(DocType::Spec, None)
            .prompt
            .starts_with("Read the attached intent.md"));
    }

    #[test]
    fn empty_skeleton_fails_validation() {
        // A fresh skeleton must not pass submit validation.
        for d in DocType::ALL {
            let t = resolve(d, None);
            assert!(!content::validate(d, &t.skeleton, &[]).is_empty());
        }
    }

    #[test]
    fn project_override() {
        let dir = tempfile::tempdir().unwrap();
        let tdir = dir.path().join(TEMPLATES_DIR);
        std::fs::create_dir_all(&tdir).unwrap();
        std::fs::write(tdir.join("plan.md"), "# Plan: <title>\n## Files\n").unwrap();
        let t = resolve(DocType::Plan, Some(dir.path()));
        assert_eq!(t.source, "project");
        assert!(t.template_ref.starts_with("plan@project:"));
        assert_eq!(render_skeleton(&t, "X"), "# Plan: X\n## Files\n");
        assert_eq!(resolve(DocType::Spec, Some(dir.path())).source, "builtin");
    }

    const PLAN_WITH_FM: &str = "---\nsymphony_template:\n  prompt: |\n    Write a short plan.\n  checklist:\n    - 是否列出回滚步骤？\n---\n# Plan: <title>\n\n## Files\n\n## Steps\n\n## Tests\n\n## Risks\n\n## Alternatives considered\n";

    #[test]
    fn front_matter_overrides_prompt_and_checklist() {
        let t = from_project_text(DocType::Plan, PLAN_WITH_FM).unwrap();
        assert_eq!(t.source, "project");
        assert_eq!(t.prompt, "Write a short plan.\n");
        assert_eq!(t.checklist, vec!["是否列出回滚步骤？".to_string()]);
        assert!(t.skeleton.starts_with("# Plan: <title>"), "{}", t.skeleton);
        assert!(!t.skeleton.contains("symphony_template"));
        assert!(t.template_ref.starts_with("plan@project:"));

        // Only the prompt: checklist stays built-in.
        let only_prompt = "---\nsymphony_template:\n  prompt: P\n---\n# Plan\n";
        let t = from_project_text(DocType::Plan, only_prompt).unwrap();
        assert_eq!(t.prompt, "P");
        assert_eq!(t.checklist, builtin(DocType::Plan).checklist);

        // No front matter: skeleton only (P1 behaviour).
        let t = from_project_text(DocType::Plan, "# Plan\n## Files\n").unwrap();
        assert_eq!(t.prompt, builtin(DocType::Plan).prompt);
        assert_eq!(t.skeleton, "# Plan\n## Files\n");
    }

    #[test]
    fn resolve_reads_front_matter_from_the_repository() {
        let dir = tempfile::tempdir().unwrap();
        let tdir = dir.path().join(TEMPLATES_DIR);
        std::fs::create_dir_all(&tdir).unwrap();
        std::fs::write(tdir.join("plan.md"), PLAN_WITH_FM).unwrap();
        let t = resolve(DocType::Plan, Some(dir.path()));
        assert_eq!(t.prompt, "Write a short plan.\n");
        assert!(t.skeleton.starts_with("# Plan: <title>"));
    }

    #[test]
    fn validation_reports_bad_front_matter_and_missing_sections() {
        assert!(validate_template(DocType::Plan, PLAN_WITH_FM).is_empty());
        let bad_yaml = "---\nsymphony_template: [\n---\n# Plan\n";
        assert!(validate_template(DocType::Plan, bad_yaml)[0].contains("front matter"));
        let bad_list = "---\nsymphony_template:\n  checklist: nope\n---\n# Plan\n";
        assert!(validate_template(DocType::Plan, bad_list)[0].contains("front matter"));
        let missing = validate_template(DocType::Plan, "# Plan\n\n## Files\n");
        assert!(
            missing.iter().any(|e| e.contains("## Steps")),
            "{missing:?}"
        );
        assert!(!validate_template(DocType::Plan, "  \n").is_empty());
    }

    #[test]
    fn editable_text_round_trips() {
        for d in DocType::ALL {
            let b = builtin(d);
            let text = editable_text(&b);
            let t = from_project_text(d, &text).unwrap();
            assert_eq!(
                (t.prompt, t.checklist, t.skeleton),
                (b.prompt, b.checklist, b.skeleton)
            );
            assert!(validate_template(d, &text).is_empty());
        }
    }
}
