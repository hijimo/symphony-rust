//! Document templates (§5): built-in defaults based on the Playbook, optionally
//! overridden by `docs/changes/_templates/<doc>.md` in the primary repository.
//! Organisation-level templates (codex-org-config) are not in phase 1.

use std::path::Path;

use serde::Serialize;

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

const PLAN_SKELETON: &str = "# Plan: <title>\n\n## Files\n<!-- 每行一个路径，与实现阶段工作区相对路径一致；多仓库项目以仓库别名开头 -->\n\n## Steps\n<!-- 工作顺序 -->\n\n## Tests\n<!-- 证明改动有效的测试 -->\n\n## Risks\n<!-- 可能弄坏什么、风险最大的一步 -->\n\n## Alternatives considered\n<!-- 放弃了哪些方案、为什么 -->\n";

const INTENT_PROMPT: &str = "Help the proposer write intent.md in their own words: what they want, why, under which constraints, and what is still open. Ask the questions an analyst would ask (scope, users, constraints, what success looks like). Keep the section structure of the template.";

/// Playbook Design-stage prompt (verbatim), plus Symphony output rules.
const SPEC_PROMPT: &str = "Read the attached intent.md and produce a requirements and design spec for integrating it into our existing codebase. Apply the skills available to you so the plan conforms to our brand guidelines, security policies and UX standards. Document the spec fully as spec.md, ready to hand to the engineering team. Describe clearly any areas of concern, especially where you cannot satisfy contradicting policies.";

const PLAN_PROMPT: &str = "Read the approved intent.md (if any) and spec.md and produce an implementation plan as plan.md: which files to change (in ## Files, one workspace-relative path per line), the order of work, the tests that prove it works, the risks, and the alternatives you rejected. Iterate until an engineer who never saw this conversation could implement the change from the plan alone.";

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

/// Resolve the effective template: project override when
/// `<repo_root>/docs/changes/_templates/<doc>.md` exists, else built-in.
/// A project override replaces the skeleton only; prompt and checklist stay.
pub fn resolve(doc: DocType, repo_root: Option<&Path>) -> DocTemplate {
    let mut tpl = builtin(doc);
    if let Some(root) = repo_root {
        let path = root.join(TEMPLATES_DIR).join(doc.file_name());
        if let Ok(text) = std::fs::read_to_string(&path) {
            if !text.trim().is_empty() {
                let hash = super::content::content_version(text.as_bytes());
                tpl.source = "project".to_string();
                tpl.template_ref = format!("{doc}@project:{}", &hash[..8]);
                tpl.skeleton = text;
            }
        }
    }
    tpl
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
}
