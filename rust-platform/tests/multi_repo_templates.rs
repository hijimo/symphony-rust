//! The web-platform workflow templates must (a) render byte-identically to
//! the pre-multi-repo prompt in single-repo mode and (b) render the
//! multi-repo instructions when repos are configured (design §6).

use symphony_platform::prompt::{BlockerContext, IssueContext, PromptEngine, RepoContext};

const TEMPLATES: &[(&str, &str)] = &[
    (
        "gitlab",
        include_str!("../../web-platform/src/templates/workflow_gitlab.md"),
    ),
    (
        "github",
        include_str!("../../web-platform/src/templates/workflow_github.md"),
    ),
    (
        "gitea",
        include_str!("../../web-platform/src/templates/workflow_gitea.md"),
    ),
];

/// Prompt body = everything after the second `---` of the front matter.
fn body(tpl: &str) -> &str {
    let rest = tpl.strip_prefix("---\n").unwrap();
    // Closing delimiter may follow a placeholder (`{{codex_section}}---`).
    let end = rest.find("---\n").unwrap();
    &rest[end + 4..]
}

/// Templates as committed before multi-repo support (snapshot of `main`).
const ORIGINALS: &[(&str, &str)] = &[
    (
        "gitlab",
        include_str!("fixtures/templates_pre_multi_repo/workflow_gitlab.md"),
    ),
    (
        "github",
        include_str!("fixtures/templates_pre_multi_repo/workflow_github.md"),
    ),
    (
        "gitea",
        include_str!("fixtures/templates_pre_multi_repo/workflow_gitea.md"),
    ),
];

fn issue() -> IssueContext {
    IssueContext {
        id: "1375".into(),
        identifier: "#32".into(),
        title: "添加售后支持菜单".into(),
        description: Some("desc".into()),
        priority: None,
        state: "Todo".into(),
        branch_name: Some("issue-32".into()),
        url: Some("http://h/g/r/-/issues/32".into()),
        labels: vec!["Todo".into()],
        blocked_by: Vec::<BlockerContext>::new(),
        created_at: None,
        updated_at: None,
    }
}

/// Substitute the web-layer `{{x}}` tokens the way templates/mod.rs does.
fn web_render(text: &str) -> String {
    text.replace("{{default_branch}}", "dev")
        .replace("{{platform_endpoint}}", "http://h/api/v4")
        .replace("{{project_slug}}", "zimei10525/jst-ai-studio")
        .replace("{{testing_status_map}}", "")
        .replace("{{testing_gate_section}}", "")
        .replace("{{testing_fail_minor_section}}", "")
}

#[test]
fn single_repo_prompt_is_byte_identical_to_original_template() {
    for ((name, tpl), (_, orig)) in TEMPLATES.iter().zip(ORIGINALS) {
        let current = web_render(body(tpl));
        let original = web_render(body(orig));
        let a = PromptEngine::compile(&current)
            .unwrap()
            .render(&issue(), None, 1, 20)
            .unwrap();
        let b = PromptEngine::compile(&original)
            .unwrap()
            .render(&issue(), None, 1, 20)
            .unwrap();
        assert_eq!(a, b, "{name}: single-repo prompt changed");
    }
}

#[test]
fn multi_repo_prompt_renders_repo_list_and_rules() {
    let repos = vec![
        RepoContext {
            alias: "frontend".into(),
            slug: "zimei10525/jst-ai-studio".into(),
            path: "frontend".into(),
            role: "primary".into(),
            writable: true,
            branch: Some("symphony/jst-ai-studio-32".into()),
            default_branch: "dev".into(),
            merge_after: vec!["backend".into()],
        },
        RepoContext {
            alias: "backend".into(),
            slug: "JstAi/jxy".into(),
            path: "backend".into(),
            role: "secondary".into(),
            writable: false,
            branch: None,
            default_branch: "pre".into(),
            merge_after: vec![],
        },
    ];
    for (name, tpl) in TEMPLATES {
        let current = web_render(body(tpl));
        let out = PromptEngine::compile(&current)
            .unwrap()
            .render_with_repos(&issue(), None, 1, 20, &repos)
            .unwrap();
        assert!(out.contains("`frontend/` -> `zimei10525/jst-ai-studio` (primary, writable, branch `symphony/jst-ai-studio-32`, base `dev`, merge after: backend)"), "{name}:\n{out}");
        assert!(out.contains("`backend/` -> `JstAi/jxy` (secondary, read-only, base `pre`, never commit or push)"), "{name}");
        assert!(
            out.contains("Symphony-Issue: zimei10525/jst-ai-studio#<iid>"),
            "{name}"
        );
        assert!(
            out.contains("Related to zimei10525/jst-ai-studio#<iid>"),
            "{name}"
        );
        assert!(out.contains("do NOT merge anything"), "{name}");
        assert!(
            !out.contains("open and follow `.codex/skills/land/SKILL.md`, then"),
            "{name}: land step still active"
        );
        assert!(out.contains("Do not create branches yourself"), "{name}");
        if *name == "gitlab" {
            assert!(
                out.contains("projects/zimei10525%2Fjst-ai-studio/"),
                "{name}: url_encode"
            );
        }
    }
}
