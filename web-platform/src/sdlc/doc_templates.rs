//! Document template settings (docs/方案/ai-native-sdlc-p2-design.md §5 A7):
//! effective templates read from the default branch, saving through an MR
//! (a person merges it) and trial drafting in a throwaway workspace.

use std::sync::atomic::{AtomicI64, Ordering};

use serde::Serialize;
use serde_json::{json, Value};

use super::agent::{Mode, SessionSpec};
use super::model::{DocType, TEMPLATES_DIR};
use super::service::{self, project_workspace_spec};
use super::templates;
use super::workspace::WorkspaceSpec;
use crate::error::WebPlatformError;
use crate::models::Project;
use crate::services::git_platform::CreateMergeRequest;
use crate::AppState;

const MAX_SAMPLE_CHARS: usize = 20_000;

/// Trial sessions use negative flow ids so they never collide with flows.
static TRIAL_FLOW_ID: AtomicI64 = AtomicI64::new(-1);

fn stamp() -> String {
    chrono::Utc::now().format("%Y%m%d%H%M%S%3f").to_string()
}

fn template_path(doc: DocType) -> String {
    format!("{TEMPLATES_DIR}/{}", doc.file_name())
}

/// Workspace-relative form of a primary-repository path.
fn ws_rel(spec: &WorkspaceSpec, path_in_repo: &str) -> String {
    if spec.is_multi_repo() {
        format!("{}/{path_in_repo}", spec.primary_rel())
    } else {
        path_in_repo.to_string()
    }
}

fn check(doc: DocType, content: &str) -> Result<templates::DocTemplate, WebPlatformError> {
    let errors = templates::validate_template(doc, content);
    if !errors.is_empty() {
        return Err(WebPlatformError::BadRequest(errors.join("；")));
    }
    templates::from_project_text(doc, content).map_err(WebPlatformError::BadRequest)
}

/// Effective templates with their source and editable text (`content`).
pub async fn effective(app: &AppState, project: &Project) -> Result<Vec<Value>, WebPlatformError> {
    let spec = project_workspace_spec(app, project, "doc-templates", "unused").await?;
    let mut out = Vec::new();
    for doc in DocType::ALL {
        let raw = app
            .sdlc
            .workspaces
            .read_default_branch_file(&spec, &template_path(doc))
            .await?;
        let project_tpl = raw.as_deref().and_then(|t| templates::resolve_text(doc, t));
        let (tpl, content) = match (project_tpl, raw) {
            (Some(t), Some(raw)) => (t, raw),
            _ => {
                let t = templates::resolve(doc, None);
                let text = templates::editable_text(&t);
                (t, text)
            }
        };
        let mut v = serde_json::to_value(&tpl).unwrap_or_default();
        v["content"] = json!(content);
        v["path"] = json!(template_path(doc));
        out.push(v);
    }
    Ok(out)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedTemplate {
    pub branch: String,
    pub mr_iid: u64,
    pub mr_url: String,
}

/// Commit the template on `symphony/doc-templates-<stamp>` and open an MR.
pub async fn save(
    app: &AppState,
    project: &Project,
    user_id: i64,
    doc: DocType,
    content: &str,
) -> Result<SavedTemplate, WebPlatformError> {
    check(doc, content)?;
    let ts = stamp();
    let branch = format!("symphony/doc-templates-{ts}");
    let spec =
        project_workspace_spec(app, project, &format!("doc-templates-{ts}"), &branch).await?;
    let result = save_in(app, project, user_id, doc, content, &spec).await;
    let _ = app.sdlc.workspaces.remove(&spec).await;
    result
}

async fn save_in(
    app: &AppState,
    project: &Project,
    user_id: i64,
    doc: DocType,
    content: &str,
    spec: &WorkspaceSpec,
) -> Result<SavedTemplate, WebPlatformError> {
    let ws = &app.sdlc.workspaces;
    ws.ensure(spec).await?;
    let writable = vec![ws_rel(spec, TEMPLATES_DIR)];
    ws.write_file(
        spec,
        &ws_rel(spec, &template_path(doc)),
        content,
        None,
        &writable,
    )
    .await?;
    let who = service::username(app, user_id).await;
    let committed = ws
        .commit_and_push(
            spec,
            TEMPLATES_DIR,
            &format!("docs: update {doc} document template"),
            &format!("human ({who})"),
            &[],
        )
        .await?;
    if committed.is_none() {
        return Err(WebPlatformError::BadRequest(
            "模板内容与默认分支相同，无需保存".into(),
        ));
    }
    let token = service::project_token(app, project).await?;
    let client = service::client(app, project).await?;
    let mr = client
        .create_merge_request(
            &token,
            &service::project_path(project),
            &CreateMergeRequest {
                source_branch: spec.branch.clone(),
                target_branch: project.default_branch.clone(),
                title: format!("docs: 更新 {doc} 文档模板"),
                description: Some(format!(
                    "由 {who} 在 Symphony 文档模板设置页提交，修改 `{}`。合并后新起草的 {doc} 使用该模板。",
                    template_path(doc)
                )),
                draft: false,
            },
        )
        .await
        .map_err(|e| WebPlatformError::ExternalService(format!("创建 MR 失败：{e}")))?;
    Ok(SavedTemplate {
        branch: spec.branch.clone(),
        mr_iid: mr.iid,
        mr_url: mr.web_url,
    })
}

#[derive(Debug, Serialize)]
pub struct TrialResult {
    pub status: String,
    pub error: Option<String>,
    pub content: String,
}

/// Draft once with `content` as the template in a throwaway workspace.
/// Nothing is committed or pushed; the session counts against the drafting
/// quota (it waits in the queue like any non-interactive draft).
pub async fn trial(
    app: &AppState,
    project: &Project,
    doc: DocType,
    content: &str,
    sample: &str,
) -> Result<TrialResult, WebPlatformError> {
    let tpl = check(doc, content)?;
    let sample = sample.trim();
    if sample.is_empty() || sample.chars().count() > MAX_SAMPLE_CHARS {
        return Err(WebPlatformError::BadRequest(
            "示例需求长度需为 1-20000 字".into(),
        ));
    }
    let ts = stamp();
    let spec = project_workspace_spec(
        app,
        project,
        &format!("template-trial-{ts}"),
        &format!("symphony/template-trial-{ts}"),
    )
    .await?;
    let result = trial_in(app, project, doc, &tpl, sample, &spec).await;
    let _ = app.sdlc.workspaces.remove(&spec).await;
    result
}

async fn trial_in(
    app: &AppState,
    project: &Project,
    doc: DocType,
    tpl: &templates::DocTemplate,
    sample: &str,
    spec: &WorkspaceSpec,
) -> Result<TrialResult, WebPlatformError> {
    let s = &app.sdlc;
    s.workspaces.ensure(spec).await?;
    let writable = service::writable_dir(spec, 0);
    let rel = service::ws_doc_path(spec, 0, doc);
    s.workspaces
        .write_file(
            spec,
            &rel,
            &templates::render_skeleton(tpl, "示例需求"),
            None,
            std::slice::from_ref(&writable),
        )
        .await?;
    let flow_id = TRIAL_FLOW_ID.fetch_sub(1, Ordering::SeqCst);
    let session = s.agents.get_or_create(SessionSpec {
        flow_id,
        stage: doc,
        cwd: s.workspaces.workspace_dir(project.id, &spec.key),
        writable_rel: writable.clone(),
        developer_instructions: service::developer_instructions(&writable, doc),
        thread_id: None,
        first_turn_prefix: None,
    });
    session.try_begin(Mode::NonInteractive);
    let prompt = format!(
        "{}\n\nThis is a trial run of the template; nothing is committed. Requirement to document:\n{sample}",
        service::draft_prompt(doc, &writable, None, &tpl.prompt)
    );
    let outcome = session.run_turn(&prompt).await;
    session.end();
    s.agents.remove_flow(flow_id).await;
    let text = s
        .workspaces
        .read_file(spec, &rel, &[])
        .await
        .ok()
        .and_then(|f| f.content)
        .unwrap_or_default();
    Ok(TrialResult {
        status: outcome.status,
        error: outcome.error,
        content: text,
    })
}
