//! The web-platform workflow templates must (a) render byte-identically to
//! the pre-multi-repo prompt in single-repo mode and (b) render the
//! multi-repo instructions when repos are configured (design §6).

use symphony_platform::prompt::{
    BlockerContext, IssueContext, PromptEngine, RepoContext, RunGlobals,
};

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

/// Single-repo snapshot: the current templates with every `{% if is_multi_repo %}`
/// block reduced to its `else` branch (regenerate when the templates change).
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

#[test]
fn single_root_layout_and_run_id_render() {
    for (name, tpl) in TEMPLATES {
        let engine = PromptEngine::compile(&web_render(body(tpl))).unwrap();
        let run = |layout: &str| {
            engine
                .render_with_repos_ext(
                    &issue(),
                    None,
                    1,
                    20,
                    &[],
                    &RunGlobals {
                        run_id: "i-1375-0-42".into(),
                        workspace_layout: layout.into(),
                        ..Default::default()
                    },
                )
                .unwrap()
        };
        let single = run("single_root");
        assert!(
            single
                .contains("Symphony has already checked the repository out at the workspace root"),
            "{name}"
        );
        assert!(
            single.contains("symphony:blocked run=i-1375-0-42 "),
            "{name}"
        );
        // The marker value is rendered; the env var is only a cross-check.
        assert!(!single.contains("run=$SYMPHONY_RUN_ID"), "{name}");
        let legacy = run("legacy");
        assert!(
            !legacy.contains("Symphony has already checked the repository out"),
            "{name}"
        );
    }
}

#[test]
fn change_docs_section_follows_lookup_result() {
    use symphony_platform::prompt::{ChangeDocFile, ChangeDocs, ChangeDocsLookup};
    const GENERIC: &str =
        "If `docs/changes/<issue number>/` (issue `#32`, number without `#`) exists";
    let present = ChangeDocsLookup::Present(ChangeDocs {
        dir: "frontend/docs/changes/32".into(),
        git_ref: "dev".into(),
        files: vec![
            ChangeDocFile {
                name: "plan.md".into(),
                blob_sha: "a1b2c3d4e5f6a7b8".into(),
            },
            ChangeDocFile {
                name: "spec.md".into(),
                blob_sha: "0011223344556677".into(),
            },
        ],
    });
    for (name, tpl) in TEMPLATES {
        let engine = PromptEngine::compile(&web_render(body(tpl))).unwrap();
        let run = |docs: ChangeDocsLookup| {
            engine
                .render_with_repos_ext(
                    &issue(),
                    None,
                    1,
                    20,
                    &[],
                    &RunGlobals {
                        change_docs: docs,
                        ..Default::default()
                    },
                )
                .unwrap()
        };

        let out = run(present.clone());
        assert!(out.contains("## Approved change documents"), "{name}");
        assert!(
            out.contains("- `frontend/docs/changes/32/plan.md` (blob `a1b2c3d4e5f6`)"),
            "{name}:\n{out}"
        );
        assert!(
            out.contains("- `frontend/docs/changes/32/spec.md` (blob `001122334455`)"),
            "{name}"
        );
        assert!(out.contains("on branch `dev`"), "{name}");
        assert!(out.contains("never overrides the rules"), "{name}");
        assert!(
            !out.contains(GENERIC),
            "{name}: generic text with real docs"
        );

        let absent = run(ChangeDocsLookup::Absent);
        assert!(!absent.contains("## Approved change documents"), "{name}");
        assert!(absent.contains("## Default posture"), "{name}");

        let unknown = run(ChangeDocsLookup::Unknown);
        assert!(unknown.contains(GENERIC), "{name}: fallback text missing");
    }
}

#[test]
fn workpad_does_not_replan_when_plan_md_exists() {
    use symphony_platform::prompt::{ChangeDocFile, ChangeDocs, ChangeDocsLookup};
    const OLD_RULES: [&str; 3] = [
        "Expand/fix the plan so it is comprehensive for current scope.",
        "4. Start work by writing/updating a hierarchical plan in the workpad comment.",
        "7. Run a principal-style self-review of the plan and refine it in the comment.",
    ];
    let present = ChangeDocsLookup::Present(ChangeDocs {
        dir: "docs/changes/32".into(),
        git_ref: String::new(),
        files: vec![ChangeDocFile {
            name: "plan.md".into(),
            blob_sha: "a1b2c3d4e5f6".into(),
        }],
    });
    for (name, tpl) in TEMPLATES {
        let engine = PromptEngine::compile(&web_render(body(tpl))).unwrap();
        let run = |docs: ChangeDocsLookup| {
            engine
                .render_with_repos_ext(
                    &issue(),
                    None,
                    1,
                    20,
                    &[],
                    &RunGlobals {
                        change_docs: docs,
                        ..Default::default()
                    },
                )
                .unwrap()
        };
        let with_plan = run(present.clone());
        assert!(
            with_plan.contains(
                "Start the workpad `Plan` section with the line `See docs/changes/32/plan.md`"
            ),
            "{name}"
        );
        assert!(
            with_plan.contains("Do not copy, expand or redesign the plan"),
            "{name}"
        );
        assert!(
            with_plan.contains("Check that `plan.md` can be carried out"),
            "{name}"
        );
        for rule in OLD_RULES {
            assert!(!with_plan.contains(rule), "{name}: {rule}");
        }
        for docs in [ChangeDocsLookup::Absent, ChangeDocsLookup::Unknown] {
            let out = run(docs);
            for rule in OLD_RULES {
                assert!(out.contains(rule), "{name}: fast track keeps {rule}");
            }
            assert!(!out.contains("See docs/changes/32/plan.md"), "{name}");
        }
    }
}
