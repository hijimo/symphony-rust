//! Document template settings (P2 design §5 A7): effective templates from
//! the default branch, saving through an MR and trial drafting.

mod sdlc_support;

use reqwest::StatusCode;
use sdlc_support::*;
use serde_json::{json, Value};

const PLAN_TPL: &str = "---\nsymphony_template:\n  prompt: |\n    Write a short plan for the sample requirement.\n  checklist:\n    - 是否列出回滚步骤？\n---\n# Plan: <title>\n\n## Files\n\n## Steps\n\n## Tests\n\n## Risks\n\n## Alternatives considered\n";

fn templates_url(app: &SdlcApp) -> String {
    format!("/api/projects/{}/doc-templates", app.project_id)
}

fn find<'a>(list: &'a Value, doc: &str) -> &'a Value {
    list.as_array()
        .unwrap()
        .iter()
        .find(|t| t["docType"] == doc)
        .unwrap_or_else(|| panic!("{doc} missing: {list}"))
}

/// Commit a file straight to `main` of the fake remote.
fn push_main(app: &SdlcApp, path: &str, content: &str) {
    let seed = app.dir.path().join("seed-push");
    let _ = std::fs::remove_dir_all(&seed);
    git(
        app.dir.path(),
        &[
            "clone",
            "-q",
            app.bare.to_str().unwrap(),
            seed.to_str().unwrap(),
        ],
    )
    .unwrap();
    let file = seed.join(path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, content).unwrap();
    git(&seed, &["add", "."]).unwrap();
    git(
        &seed,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "tpl",
        ],
    )
    .unwrap();
    git(&seed, &["push", "-q", "origin", "main"]).unwrap();
}

#[tokio::test]
async fn effective_templates_come_from_the_default_branch() {
    let app = SdlcApp::new().await;
    let (st, v) = app.get(&templates_url(&app)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let plan = find(&v["data"], "plan");
    assert_eq!(plan["source"], "builtin");
    // Editable text carries prompt and checklist in front matter.
    let content = plan["content"].as_str().unwrap();
    assert!(content.starts_with("---\nsymphony_template:"), "{content}");
    assert!(content.contains("## Alternatives considered"));

    push_main(&app, "docs/changes/_templates/plan.md", PLAN_TPL);
    let (_, v) = app.get(&templates_url(&app)).await;
    let plan = find(&v["data"], "plan");
    assert_eq!(plan["source"], "project", "{plan}");
    assert_eq!(plan["content"], PLAN_TPL);
    assert_eq!(
        plan["prompt"],
        "Write a short plan for the sample requirement.\n"
    );
    assert_eq!(plan["checklist"], json!(["是否列出回滚步骤？"]));
    assert!(plan["skeleton"]
        .as_str()
        .unwrap()
        .starts_with("# Plan: <title>"));
    assert_eq!(find(&v["data"], "spec")["source"], "builtin");
}

#[tokio::test]
async fn saving_a_template_opens_an_mr_with_the_file() {
    let app = SdlcApp::new().await;
    let (st, v) = app
        .put(
            &format!("{}/plan", templates_url(&app)),
            json!({"content": "# Plan\n\n## Files\n"}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["retMsg"].as_str().unwrap().contains("## Steps"), "{v}");

    let (st, v) = app
        .put(
            &format!("{}/plan", templates_url(&app)),
            json!({"content": PLAN_TPL}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let branch = v["data"]["branch"].as_str().unwrap().to_string();
    assert!(branch.starts_with("symphony/doc-templates-"), "{v}");
    assert!(v["data"]["mrUrl"]
        .as_str()
        .unwrap()
        .contains("merge_requests"));
    assert_eq!(
        read_branch_file(&app.bare, &branch, "docs/changes/_templates/plan.md").as_deref(),
        Some(PLAN_TPL)
    );
    let mr = app.gitlab.mr(v["data"]["mrIid"].as_u64().unwrap());
    assert_eq!(mr.source, branch);
    // main is untouched until a person merges the MR.
    assert!(read_branch_file(&app.bare, "main", "docs/changes/_templates/plan.md").is_none());
}

#[tokio::test]
async fn trial_draft_returns_the_document_without_committing() {
    let app = SdlcApp::new().await;
    let (st, v) = app
        .post(
            &format!("{}/plan/trial", templates_url(&app)),
            json!({"content": PLAN_TPL, "sample": "用户希望导出报表"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let out = v["data"]["content"].as_str().unwrap();
    assert!(out.contains("## Steps"), "{v}");
    assert_eq!(v["data"]["status"], "completed");

    let log = std::fs::read_to_string(app.codex_log()).unwrap_or_default();
    assert!(log.contains("thread/start"), "{log}");
    let branches = git(&app.bare, &["branch", "--list", "symphony/*"]).unwrap();
    assert!(branches.trim().is_empty(), "nothing pushed: {branches}");
    let trial_dirs: Vec<_> = std::fs::read_dir(
        app.workspace_root
            .join("editor")
            .join(app.project_id.to_string()),
    )
    .map(|d| {
        d.filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("template-trial"))
            .collect()
    })
    .unwrap_or_default();
    assert!(trial_dirs.is_empty(), "workspace removed: {trial_dirs:?}");

    let (st, _) = app
        .post(
            &format!("{}/plan/trial", templates_url(&app)),
            json!({"content": PLAN_TPL, "sample": ""}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}
