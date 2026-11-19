//! Document templates (§5): built-in defaults based on the Playbook, optionally
//! overridden by `docs/changes/_templates/<doc>.md` in the primary repository.
//! Between the two sits the organization rules repository's
//! `templates/<doc>.md` (P3 design §4 O3).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::{DocType, TEMPLATES_DIR};

/// Built-in template version per template. Bump the template's version
/// when its built-in skeleton changes. Intent v2 adds `## Value` and keeps
/// the document at the business level.
pub fn builtin_version(key: TemplateKey) -> u32 {
    match key {
        TemplateKey {
            doc: DocType::Intent,
            incident: false,
        } => 2,
        _ => 1,
    }
}

/// A template: one per document type, plus the incident variant of intent
/// (P3 design §3 S4). The variant reuses the intent document, gate and
/// labels; only skeleton, prompt and checklist differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateKey {
    pub doc: DocType,
    pub incident: bool,
}

impl From<DocType> for TemplateKey {
    fn from(doc: DocType) -> Self {
        Self {
            doc,
            incident: false,
        }
    }
}

impl TemplateKey {
    pub const INCIDENT: TemplateKey = TemplateKey {
        doc: DocType::Intent,
        incident: true,
    };
    pub const ALL: [TemplateKey; 4] = [
        TemplateKey {
            doc: DocType::Intent,
            incident: false,
        },
        TemplateKey::INCIDENT,
        TemplateKey {
            doc: DocType::Spec,
            incident: false,
        },
        TemplateKey {
            doc: DocType::Plan,
            incident: false,
        },
    ];

    /// `intent`, `spec`, `plan` or `intent.incident`.
    pub fn parse(s: &str) -> Result<Self, crate::error::WebPlatformError> {
        match s.strip_suffix(".incident") {
            Some("intent") => Ok(Self::INCIDENT),
            Some(_) => Err(crate::error::WebPlatformError::BadRequest(format!(
                "unknown template: {s}"
            ))),
            None => Ok(s.parse::<DocType>()?.into()),
        }
    }

    pub fn as_str(self) -> String {
        if self.incident {
            format!("{}.incident", self.doc)
        } else {
            self.doc.to_string()
        }
    }

    pub fn file_name(self) -> String {
        format!("{}.md", self.as_str())
    }

    /// The key a document was created with, from its `template_ref`.
    pub fn of_template_ref(doc: DocType, template_ref: &str) -> Self {
        let incident = doc == DocType::Intent && template_ref.starts_with("intent.incident@");
        Self { doc, incident }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DocTemplate {
    pub doc_type: DocType,
    /// `intent`, `spec`, `plan` or `intent.incident`.
    pub key: String,
    /// `builtin`, `org` or `project`.
    pub source: String,
    /// e.g. `spec@1`, `spec@org:<sha8>` or `spec@project:<hash8>`.
    pub template_ref: String,
    pub skeleton: String,
    pub prompt: String,
    pub checklist: Vec<String>,
}

const INTENT_SKELETON: &str = "# Intent: <title>\n\n## Problem\n<!-- 用提出者自己的话：今天做不到什么、谁受影响。不需要正式语言，不写技术方案 -->\n\n## Value\n<!-- 为什么值得做：谁受益、收益多大（省多少时间或钱、多少用户受益、带来多少收入）、为什么是现在。能量化就量化，不能量化就写依据 -->\n\n## Proposed outcome\n<!-- 做成之后用户看到的状态，以及怎么判断做成了。写结果，不写实现 -->\n\n## Affected users and systems\n<!-- 受影响的用户、角色和业务流程。不写接口、数据表、代码模块 -->\n\n## Constraints\n<!-- 业务约束：截止时间、合规要求、预算、不能影响的现有业务等 -->\n\n## Open questions\n<!-- 还没想清楚、需要产品负责人在审阅时判断的问题 -->\n";

const SPEC_SKELETON: &str = "# Spec: <title>\n\n## Requirements\n<!-- 需求：要做什么、验收标准 -->\n\n## Design\n<!-- 设计：如何集成进现有代码库，接口、数据、交互 -->\n\n## Concerns\n<!-- 关注点：每条一行，写明 owner: <负责人> resolution: <结论>；没有则写“无” -->\n";

const PLAN_SKELETON: &str = "# Plan: <title>\n\n## Files\n<!-- 每行一个 Markdown 列表项，以 \"- \" 开头，写工作区相对路径，可在路径后加说明；多仓库项目以仓库别名开头。例如：\n- src/pages/Example.tsx 新增页面\n- backend/src/api.rs 增加接口\n-->\n\n## Steps\n<!-- 工作顺序 -->\n\n## Tests\n<!-- 证明改动有效的测试 -->\n\n## Risks\n<!-- 可能弄坏什么、风险最大的一步 -->\n\n## Alternatives considered\n<!-- 放弃了哪些方案、为什么 -->\n";

const INCIDENT_SKELETON: &str = "# Intent: <title>\n\n## Problem\n<!-- 出了什么问题、谁受影响。按信号证据写，不要猜测原因 -->\n\n## Anomaly and evidence\n<!-- 异常与证据：现象、影响范围、开始时间与时间线、证据链接（告警、错误、堆栈摘要） -->\n\n## Proposed outcome\n<!-- 处理后应达到的状态：止血、修复、防止复发 -->\n\n## Affected users and systems\n<!-- 受影响的用户和系统 -->\n\n## Constraints\n<!-- 约束：变更窗口、回滚要求、合规等 -->\n\n## Open questions\n<!-- 还不确定的事：根因、影响范围、是否需要先止血 -->\n";

const INCIDENT_PROMPT: &str = "Turn the external signal below into intent.md for an incident. State the problem and its impact from the evidence only; put the observed facts (symptoms, scope, start time, timeline, links) under ## Anomaly and evidence and anything you infer or do not know under ## Open questions. Do not invent root causes. Keep every section of the template.";

const INTENT_PROMPT: &str = "Help the proposer write intent.md in their own words. The product owner reviews it to decide whether the idea is worth doing and feasible enough to enter the product. Cover the problem and who has it, the value (who benefits, how much, why now), the desired outcome and how we will know it worked, business constraints, and what is still open. Ask the questions a product manager would ask (who has the problem, how often, what it costs today, what success looks like). Stay at the business level: do not describe implementation (APIs, data tables, files, modules, architecture, technology choices) and do not assess technical feasibility; that belongs to the spec. Keep the section structure of the template.";

/// Playbook Design-stage prompt (verbatim), plus Symphony output rules.
const SPEC_PROMPT: &str = "Read the attached intent.md and produce a requirements and design spec for integrating it into our existing codebase. Apply the skills available to you so the plan conforms to our brand guidelines, security policies and UX standards. Document the spec fully as spec.md, ready to hand to the engineering team. Describe clearly any areas of concern, especially where you cannot satisfy contradicting policies.";

const PLAN_PROMPT: &str = "Read the approved intent.md (if any) and spec.md and produce an implementation plan as plan.md: which files to change (in ## Files, one Markdown bullet per file: a line starting with \"- \" followed by the workspace-relative path, e.g. \"- src/app.ts\"; for multi-repo projects start the path with the repository alias), the order of work, the tests that prove it works, the risks, and the alternatives you rejected. Iterate until an engineer who never saw this conversation could implement the change from the plan alone.";

fn builtin(key: TemplateKey) -> DocTemplate {
    let doc = key.doc;
    let (skeleton, prompt, checklist): (&str, &str, &[&str]) = match doc {
        DocType::Intent if key.incident => (
            INCIDENT_SKELETON,
            INCIDENT_PROMPT,
            &[
                "影响范围和严重度判断是否有证据支持？",
                "是否需要先止血、由谁负责？",
                "是否与已有 Issue 重复？",
                "推测的原因是否标明为推测并放在 Open questions？",
            ],
        ),
        DocType::Intent => (
            INTENT_SKELETON,
            INTENT_PROMPT,
            &[
                "问题是否用提出者自己的话写清楚了？谁有这个问题？",
                "价值是否成立：谁受益、收益多大、为什么现在做？",
                "期望结果是否具体、可判断？",
                "是否有明显做不到或代价过高的地方？",
                "是否与已有需求重复？约束和不在范围内的内容是否写明？",
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
        key: key.as_str(),
        source: "builtin".to_string(),
        template_ref: format!("{}@{}", key.as_str(), builtin_version(key)),
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
pub fn from_project_text(key: impl Into<TemplateKey>, text: &str) -> Result<DocTemplate, String> {
    let key = key.into();
    let hash = super::content::content_version(text.as_bytes());
    parse_text(
        key,
        text,
        "project",
        format!("{}@project:{}", key.as_str(), &hash[..8]),
    )
}

/// Organization template (same format as a project template) of the
/// organization rules version `sha`.
pub fn from_org_text(
    key: impl Into<TemplateKey>,
    text: &str,
    sha: &str,
) -> Result<DocTemplate, String> {
    let key = key.into();
    parse_text(
        key,
        text,
        "org",
        format!("{}@org:{}", key.as_str(), &sha[..sha.len().min(8)]),
    )
}

fn parse_text(
    key: TemplateKey,
    text: &str,
    source: &str,
    template_ref: String,
) -> Result<DocTemplate, String> {
    let mut tpl = builtin(key);
    tpl.source = source.to_string();
    tpl.template_ref = template_ref;
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
pub fn validate_template(key: impl Into<TemplateKey>, text: &str) -> Vec<String> {
    let key = key.into();
    if text.trim().is_empty() {
        return vec!["模板不能为空".to_string()];
    }
    let tpl = match from_project_text(key, text) {
        Ok(t) => t,
        Err(e) => return vec![e],
    };
    let mut out: Vec<String> = super::content::required_sections_for(key.doc, key.incident)
        .into_iter()
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

/// Organization templates of one organization rules version:
/// `<export>/templates/<key>.md` (P3 design §4 O3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgTemplates {
    pub dir: PathBuf,
    pub sha: String,
}

/// Resolve the effective template: project override when
/// `<repo_root>/docs/changes/_templates/<doc>.md` exists, else the
/// organization template, else built-in.
pub fn resolve(
    key: impl Into<TemplateKey>,
    repo_root: Option<&Path>,
    org: Option<&OrgTemplates>,
) -> DocTemplate {
    let key = key.into();
    if let Some(root) = repo_root {
        let path = root.join(TEMPLATES_DIR).join(key.file_name());
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Some(t) = resolve_text(key, &text) {
                return t;
            }
        }
    }
    resolve_org(key, org).unwrap_or_else(|| builtin(key))
}

/// Organization template, `None` when missing, empty or invalid.
pub fn resolve_org(key: impl Into<TemplateKey>, org: Option<&OrgTemplates>) -> Option<DocTemplate> {
    let key = key.into();
    let org = org?;
    let text = std::fs::read_to_string(org.dir.join(key.file_name())).ok()?;
    if text.trim().is_empty() {
        return None;
    }
    from_org_text(key, &text, &org.sha)
        .map_err(|e| {
            tracing::warn!(template = %key.as_str(), "ignoring invalid organization template: {e}");
        })
        .ok()
}

/// Project template from file text; `None` (built-in applies) when empty or
/// unparseable.
pub fn resolve_text(key: impl Into<TemplateKey>, text: &str) -> Option<DocTemplate> {
    let key = key.into();
    if text.trim().is_empty() {
        return None;
    }
    match from_project_text(key, text) {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!(template = %key.as_str(), "ignoring invalid project template: {e}");
            None
        }
    }
}

pub fn all(repo_root: Option<&Path>, org: Option<&OrgTemplates>) -> Vec<DocTemplate> {
    DocType::ALL
        .iter()
        .map(|d| resolve(*d, repo_root, org))
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
            let t = resolve(d, None, None);
            assert_eq!(t.source, "builtin");
            for req in content::required_sections(d) {
                assert!(
                    t.skeleton.contains(&format!("## {req}")),
                    "{d} missing {req}"
                );
            }
            assert!(!t.checklist.is_empty());
        }
        assert!(resolve(DocType::Spec, None, None)
            .prompt
            .starts_with("Read the attached intent.md"));
    }

    #[test]
    fn intent_v2_asks_for_value_and_stays_at_business_level() {
        let t = resolve(DocType::Intent, None, None);
        assert_eq!(t.template_ref, "intent@2");
        let problem = t.skeleton.find("## Problem").unwrap();
        let value = t.skeleton.find("## Value").unwrap();
        let outcome = t.skeleton.find("## Proposed outcome").unwrap();
        assert!(problem < value && value < outcome, "{}", t.skeleton);
        assert!(t.prompt.contains("do not describe implementation"));
        assert!(t.checklist.iter().any(|c| c.contains("价值")));
        // Other templates keep their version.
        assert_eq!(resolve(DocType::Spec, None, None).template_ref, "spec@1");
        // Custom intent templates must include Value from now on.
        let old = "# Intent\n## Problem\n## Proposed outcome\n## Affected users and systems\n## Constraints\n## Open questions\n";
        let errs = validate_template(DocType::Intent, old);
        assert_eq!(errs, ["骨架缺少必填章节 ## Value"]);
    }

    #[test]
    fn empty_skeleton_fails_validation() {
        // A fresh skeleton must not pass submit validation.
        for d in DocType::ALL {
            let t = resolve(d, None, None);
            assert!(!content::validate(d, &t.skeleton, &[]).is_empty());
        }
    }

    #[test]
    fn project_override() {
        let dir = tempfile::tempdir().unwrap();
        let tdir = dir.path().join(TEMPLATES_DIR);
        std::fs::create_dir_all(&tdir).unwrap();
        std::fs::write(tdir.join("plan.md"), "# Plan: <title>\n## Files\n").unwrap();
        let t = resolve(DocType::Plan, Some(dir.path()), None);
        assert_eq!(t.source, "project");
        assert!(t.template_ref.starts_with("plan@project:"));
        assert_eq!(render_skeleton(&t, "X"), "# Plan: X\n## Files\n");
        assert_eq!(
            resolve(DocType::Spec, Some(dir.path()), None).source,
            "builtin"
        );
    }

    #[test]
    fn organization_layer_sits_between_project_and_builtin() {
        let dir = tempfile::tempdir().unwrap();
        let org = OrgTemplates {
            dir: dir.path().join("org/templates"),
            sha: "0123456789abcdef0123456789abcdef01234567".into(),
        };
        std::fs::create_dir_all(&org.dir).unwrap();
        std::fs::write(org.dir.join("plan.md"), PLAN_WITH_FM).unwrap();
        std::fs::write(org.dir.join("intent.incident.md"), "# Incident <title>\n").unwrap();
        std::fs::write(org.dir.join("spec.md"), "  \n").unwrap();
        std::fs::write(
            org.dir.join("intent.md"),
            "---\nsymphony_template: [\n---\nx",
        )
        .unwrap();

        let t = resolve(DocType::Plan, None, Some(&org));
        assert_eq!(t.source, "org");
        assert_eq!(t.template_ref, "plan@org:01234567");
        assert_eq!(t.prompt, "Write a short plan.\n");
        assert_eq!(t.checklist, ["是否列出回滚步骤？"]);
        assert!(t.skeleton.starts_with("# Plan: <title>"));
        let t = resolve(TemplateKey::INCIDENT, None, Some(&org));
        assert_eq!(t.template_ref, "intent.incident@org:01234567");
        assert_eq!(
            TemplateKey::of_template_ref(DocType::Intent, &t.template_ref),
            TemplateKey::INCIDENT
        );
        // Empty or invalid organization templates fall back to built-in.
        assert_eq!(resolve(DocType::Spec, None, Some(&org)).source, "builtin");
        assert_eq!(resolve(DocType::Intent, None, Some(&org)).source, "builtin");

        // The project template wins over the organization one.
        let tdir = dir.path().join(TEMPLATES_DIR);
        std::fs::create_dir_all(&tdir).unwrap();
        std::fs::write(tdir.join("plan.md"), "# Plan: <title>\n## Files\n").unwrap();
        assert_eq!(
            resolve(DocType::Plan, Some(dir.path()), Some(&org)).source,
            "project"
        );
        let sources: Vec<String> = all(Some(dir.path()), Some(&org))
            .into_iter()
            .map(|t| t.source)
            .collect();
        assert_eq!(sources, ["builtin", "builtin", "project"]);
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
        assert_eq!(t.checklist, builtin(DocType::Plan.into()).checklist);

        // No front matter: skeleton only (P1 behaviour).
        let t = from_project_text(DocType::Plan, "# Plan\n## Files\n").unwrap();
        assert_eq!(t.prompt, builtin(DocType::Plan.into()).prompt);
        assert_eq!(t.skeleton, "# Plan\n## Files\n");
    }

    #[test]
    fn resolve_reads_front_matter_from_the_repository() {
        let dir = tempfile::tempdir().unwrap();
        let tdir = dir.path().join(TEMPLATES_DIR);
        std::fs::create_dir_all(&tdir).unwrap();
        std::fs::write(tdir.join("plan.md"), PLAN_WITH_FM).unwrap();
        let t = resolve(DocType::Plan, Some(dir.path()), None);
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
        for d in TemplateKey::ALL {
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

    #[test]
    fn incident_is_an_intent_variant() {
        let k = TemplateKey::parse("intent.incident").unwrap();
        assert_eq!((k.doc, k.incident), (DocType::Intent, true));
        assert_eq!(k.as_str(), "intent.incident");
        assert_eq!(k.file_name(), "intent.incident.md");
        assert!(TemplateKey::parse("spec.incident").is_err());
        assert!(TemplateKey::parse("nope").is_err());
        assert_eq!(
            TemplateKey::parse("plan").unwrap(),
            TemplateKey::from(DocType::Plan)
        );
        assert_eq!(TemplateKey::ALL.len(), 4);

        let t = resolve(TemplateKey::INCIDENT, None, None);
        assert_eq!(t.doc_type, DocType::Intent);
        assert_eq!(t.key, "intent.incident");
        assert_eq!(t.template_ref, "intent.incident@1");
        let problem = t.skeleton.find("## Problem").unwrap();
        let anomaly = t.skeleton.find("## Anomaly and evidence").unwrap();
        let outcome = t.skeleton.find("## Proposed outcome").unwrap();
        assert!(problem < anomaly && anomaly < outcome, "{}", t.skeleton);
        assert!(
            t.checklist.iter().any(|c| c.contains("止血")),
            "triage checklist"
        );
        assert_ne!(t.prompt, builtin(DocType::Intent.into()).prompt);
        assert_eq!(
            TemplateKey::of_template_ref(DocType::Intent, &t.template_ref),
            TemplateKey::INCIDENT
        );
        assert_eq!(
            TemplateKey::of_template_ref(DocType::Intent, "intent@1"),
            DocType::Intent.into()
        );
        assert_eq!(
            TemplateKey::of_template_ref(DocType::Intent, "intent.incident@project:abcd1234"),
            TemplateKey::INCIDENT
        );
    }

    #[test]
    fn incident_templates_require_the_evidence_section() {
        let k = TemplateKey::INCIDENT;
        let plain = editable_text(&builtin(DocType::Intent.into()));
        let errs = validate_template(k, &plain);
        assert!(
            errs.iter().any(|e| e.contains("Anomaly and evidence")),
            "{errs:?}"
        );
        assert!(validate_template(k, &editable_text(&builtin(k))).is_empty());
        let dir = tempfile::tempdir().unwrap();
        let tdir = dir.path().join(TEMPLATES_DIR);
        std::fs::create_dir_all(&tdir).unwrap();
        std::fs::write(tdir.join("intent.incident.md"), "# Incident\n## Problem\n").unwrap();
        let t = resolve(k, Some(dir.path()), None);
        assert_eq!(t.source, "project");
        assert!(
            t.template_ref.starts_with("intent.incident@project:"),
            "{}",
            t.template_ref
        );
        assert_eq!(
            resolve(DocType::Intent, Some(dir.path()), None).source,
            "builtin"
        );
    }
}
