//! REST + WebSocket API of the document flow and workbench.
//!
//! REST: `/api/projects/{id}/changes[/{iid}/...]`
//! WS:   `/api/projects/{id}/changes/{iid}/ws?ticket=...` (one-time ticket).

use std::collections::HashMap;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::middleware::project_access::require_project_member;
use crate::models::{Project, ResponseData};
use crate::repository::ProjectRepository;
use crate::sdlc::agent::Mode;
use crate::sdlc::model::{DocType, Track};
use crate::sdlc::service::{self, Ctx, StartRequest};
use crate::sdlc::state::GateAction;
use crate::sdlc::templates;
use crate::sdlc::workspace::WorkspaceService;
use crate::AppState;

type ApiResult = Result<Json<ResponseData<Value>>, WebPlatformError>;

fn ok(v: impl serde::Serialize) -> ApiResult {
    Ok(Json(ResponseData::success(
        serde_json::to_value(v).map_err(|e| WebPlatformError::Internal(e.to_string()))?,
    )))
}

fn user_id(claims: &Claims) -> Result<i64, WebPlatformError> {
    claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".into()))
}

async fn ctx(state: &AppState, claims: &Claims, project_id: i64) -> Result<Ctx, WebPlatformError> {
    require_project_member(claims, project_id, &state.repo).await?;
    let project: Project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".into()))?;
    Ok(Ctx {
        app: state.clone(),
        project,
        user_id: user_id(claims)?,
    })
}

fn parse_doc(s: &str) -> Result<DocType, WebPlatformError> {
    s.parse()
}

// ------------------------------------------------------------------ flows

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartBody {
    pub track: String,
    pub issue_iid: Option<i64>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub initial_content: Option<String>,
}

/// POST /api/projects/{id}/changes — start a document flow (§2 入口).
pub async fn start(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(body): Json<StartBody>,
) -> ApiResult {
    let ctx = ctx(&state, &claims, project_id).await?;
    let track = Track::parse(&body.track)?;
    if body.issue_iid.is_none()
        && body
            .title
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
    {
        return Err(WebPlatformError::BadRequest(
            "新建需求时 title 不能为空".into(),
        ));
    }
    if let Some(c) = &body.initial_content {
        if c.len() > 256 * 1024 {
            return Err(WebPlatformError::BadRequest("initialContent 过大".into()));
        }
    }
    let v = service::start_flow(
        &ctx,
        StartRequest {
            track,
            issue_iid: body.issue_iid,
            title: body.title,
            summary: body.summary,
            initial_content: body.initial_content,
        },
    )
    .await?;
    ok(v)
}

/// GET /api/projects/{id}/changes — flows of a project (for kanban cards).
pub async fn list(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> ApiResult {
    let ctx = ctx(&state, &claims, project_id).await?;
    let flows = state.sdlc.store.list_flows(project_id).await?;
    let mut out = Vec::with_capacity(flows.len());
    for f in flows {
        let docs = state.sdlc.store.list_documents(f.id).await?;
        let statuses = docs.iter().map(|d| (d.doc_type, d.status)).collect();
        let st = crate::sdlc::state::derive_flow_state(f.track, &statuses);
        out.push(json!({
            "issueIid": f.issue_iid,
            "track": f.track,
            "revision": f.revision,
            "state": st,
            "label": st.doc_label(),
            "docsMrUrl": f.docs_mr_url,
            "mergeError": f.merge_error,
            "documents": docs.iter().map(|d| json!({"docType": d.doc_type, "status": d.status, "draftState": d.draft_state})).collect::<Vec<_>>(),
            "updatedAt": f.updated_at,
        }));
    }
    let _ = ctx;
    ok(out)
}

/// GET /api/projects/{id}/changes/{iid}
pub async fn get(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, i64)>,
) -> ApiResult {
    let ctx = ctx(&state, &claims, project_id).await?;
    let flow = service::load_flow(&state, &ctx.project, iid).await?;
    let mut v = serde_json::to_value(service::view(&state, &ctx.project, flow).await?)
        .map_err(|e| WebPlatformError::Internal(e.to_string()))?;
    let pre = service::merge_precheck(&ctx).await;
    v["mergePrecheck"] = json!({"canMerge": pre.can_merge, "reason": pre.reason});
    ok(v)
}

/// POST /api/projects/{id}/changes/{iid}/workspace — create / refresh the
/// workbench workspace and report context completeness.
pub async fn ensure_workspace(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, i64)>,
) -> ApiResult {
    let ctx = ctx(&state, &claims, project_id).await?;
    let flow = service::load_flow(&state, &ctx.project, iid).await?;
    let spec = service::workspace_spec(&state, &ctx.project, &flow).await?;
    let status = state.sdlc.workspaces.ensure(&spec).await?;
    ok(json!({
        "context": status,
        "writableDir": service::writable_dir(&spec, iid),
        "multiRepo": spec.is_multi_repo(),
        "primaryDir": spec.primary_rel(),
    }))
}

/// POST /api/projects/{id}/changes/{iid}/workspace/merge-main
pub async fn merge_main(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, i64)>,
) -> ApiResult {
    let ctx = ctx(&state, &claims, project_id).await?;
    let flow = service::load_flow(&state, &ctx.project, iid).await?;
    let lock = state.sdlc.flow_lock(flow.id);
    let _g = lock.lock().await;
    let spec = service::workspace_spec(&state, &ctx.project, &flow).await?;
    state.sdlc.workspaces.merge_main(&spec).await?;
    state.sdlc.workspaces.push(&spec).await?;
    state.sdlc.notify(flow.id, json!({"type": "tree_changed"}));
    ok(json!({"merged": true}))
}

// ----------------------------------------------------------------- files

#[derive(Debug, Deserialize)]
pub struct PathQuery {
    #[serde(default)]
    pub path: String,
    /// Optional commit sha: read the file at that revision.
    pub rev: Option<String>,
}

async fn flow_spec(
    state: &AppState,
    claims: &Claims,
    project_id: i64,
    iid: i64,
) -> Result<
    (
        Ctx,
        crate::sdlc::store::ChangeFlow,
        crate::sdlc::workspace::WorkspaceSpec,
        Vec<String>,
    ),
    WebPlatformError,
> {
    let ctx = ctx(state, claims, project_id).await?;
    let flow = service::load_flow(state, &ctx.project, iid).await?;
    let spec = service::workspace_spec(state, &ctx.project, &flow).await?;
    let writable = vec![service::writable_dir(&spec, iid)];
    Ok((ctx, flow, spec, writable))
}

/// GET .../tree?path=
pub async fn tree(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, i64)>,
    Query(q): Query<PathQuery>,
) -> ApiResult {
    let (_, _, spec, writable) = flow_spec(&state, &claims, project_id, iid).await?;
    ok(state
        .sdlc
        .workspaces
        .list_dir(&spec, &q.path, &writable)
        .await?)
}

/// GET .../file?path=
pub async fn read_file(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, i64)>,
    Query(q): Query<PathQuery>,
) -> ApiResult {
    let (_, flow, spec, writable) = flow_spec(&state, &claims, project_id, iid).await?;
    if let Some(rev) = q.rev.as_deref().filter(|r| !r.is_empty()) {
        let content = state.sdlc.workspaces.read_at(&spec, &q.path, rev).await?;
        return ok(json!({"path": q.path, "rev": rev, "content": content}));
    }
    let mut f = state
        .sdlc
        .workspaces
        .read_file(&spec, &q.path, &writable)
        .await?;
    if state.sdlc.agents.locked_paths(flow.id).contains(&q.path) {
        f.writable = false;
    }
    ok(f)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteBody {
    pub path: String,
    pub content: String,
    pub base_version: Option<String>,
    /// `true` = 用户点“保存”（commit + push）；`false` = 自动保存。
    #[serde(default)]
    pub commit: bool,
}

/// PUT .../file
pub async fn write_file(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, i64)>,
    Json(body): Json<WriteBody>,
) -> ApiResult {
    if body.content.len() > crate::sdlc::workspace::MAX_TEXT_FILE as usize {
        return Err(WebPlatformError::BadRequest("文件超过 1MB".into()));
    }
    let ctx = ctx(&state, &claims, project_id).await?;
    let flow = service::load_flow(&state, &ctx.project, iid).await?;
    let r = service::save_file(
        &ctx,
        &flow,
        &body.path,
        &body.content,
        body.base_version.as_deref(),
        body.commit,
    )
    .await?;
    ok(json!({"version": r.version, "commitSha": r.commit_sha}))
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_mode")]
    pub mode: String,
}

fn default_mode() -> String {
    "content".into()
}

/// GET .../search?q=&mode=file|content
pub async fn search(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, i64)>,
    Query(q): Query<SearchQuery>,
) -> ApiResult {
    let (_, _, spec, _) = flow_spec(&state, &claims, project_id, iid).await?;
    ok(state.sdlc.workspaces.search(&spec, &q.q, &q.mode).await?)
}

/// GET .../diff — uncommitted changes of the docs directory.
pub async fn diff(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, i64)>,
) -> ApiResult {
    let (_, _, spec, _) = flow_spec(&state, &claims, project_id, iid).await?;
    let d = state
        .sdlc
        .workspaces
        .diff(&spec, &crate::sdlc::model::issue_changes_dir(iid))
        .await?;
    ok(json!({"diff": d}))
}

/// GET .../templates — effective document templates.
pub async fn list_templates(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, i64)>,
) -> ApiResult {
    let (_, _, spec, _) = flow_spec(&state, &claims, project_id, iid).await?;
    let root = state.sdlc.workspaces.primary_dir(&spec);
    ok(templates::all(Some(&root)))
}

/// GET /api/projects/{id}/doc-templates — built-in templates (project
/// overrides are resolved inside a workspace).
pub async fn builtin_templates(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> ApiResult {
    require_project_member(&claims, project_id, &state.repo).await?;
    ok(templates::all(None))
}

/// POST .../docs/{doc}/validate — run submit validation without submitting.
pub async fn validate(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid, doc)): Path<(i64, i64, String)>,
) -> ApiResult {
    let doc = parse_doc(&doc)?;
    let (ctx, _, spec, _) = flow_spec(&state, &claims, project_id, iid).await?;
    let rel = service::ws_doc_path(&spec, iid, doc);
    let text = state
        .sdlc
        .workspaces
        .read_file(&spec, &rel, &[])
        .await?
        .content
        .unwrap_or_default();
    let readonly: Vec<String> =
        crate::services::project_repos::resolve_active_repos(&state.repo, &ctx.project)
            .await?
            .into_iter()
            .filter(|r| r.mode != crate::models::REPO_MODE_WRITE)
            .map(|r| r.alias)
            .collect();
    ok(crate::sdlc::content::validate(doc, &text, &readonly))
}

// ------------------------------------------------------------------ gates

/// POST .../docs/{doc}/submit
pub async fn submit(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid, doc)): Path<(i64, i64, String)>,
) -> ApiResult {
    let doc = parse_doc(&doc)?;
    let ctx = ctx(&state, &claims, project_id).await?;
    let flow = service::load_flow(&state, &ctx.project, iid).await?;
    ok(service::submit(&ctx, &flow, doc).await?)
}

#[derive(Debug, Deserialize)]
pub struct DecideBody {
    pub action: String,
    pub comment: Option<String>,
}

/// POST .../docs/{doc}/decision — approve / request_changes / reject.
pub async fn decide(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid, doc)): Path<(i64, i64, String)>,
    Json(body): Json<DecideBody>,
) -> ApiResult {
    let doc = parse_doc(&doc)?;
    let action = GateAction::parse(&body.action)?;
    if body.comment.as_deref().map(|c| c.len()).unwrap_or(0) > 10_000 {
        return Err(WebPlatformError::BadRequest("comment 过长".into()));
    }
    let ctx = ctx(&state, &claims, project_id).await?;
    let flow = service::load_flow(&state, &ctx.project, iid).await?;
    ok(service::decide(&ctx, &flow, doc, action, body.comment).await?)
}

/// POST .../merge — retry merging the docs MR.
pub async fn retry_merge(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, i64)>,
) -> ApiResult {
    let ctx = ctx(&state, &claims, project_id).await?;
    let flow = service::load_flow(&state, &ctx.project, iid).await?;
    let pre = service::merge_precheck(&ctx).await;
    if !pre.can_merge {
        return Err(WebPlatformError::ForbiddenWithReason(
            pre.reason.unwrap_or_default(),
        ));
    }
    service::merge_docs(&ctx, &flow).await?;
    let flow = service::load_flow(&state, &ctx.project, iid).await?;
    ok(service::view(&state, &ctx.project, flow).await?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertBody {
    pub to: String,
    pub impl_branch: Option<String>,
    pub code_mr_iid: Option<i64>,
}

/// POST .../revert — 退回文档阶段 (r2).
pub async fn revert(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, i64)>,
    Json(body): Json<RevertBody>,
) -> ApiResult {
    let to = parse_doc(&body.to)?;
    let ctx = ctx(&state, &claims, project_id).await?;
    let flow = service::load_flow(&state, &ctx.project, iid).await?;
    ok(service::revert(&ctx, &flow, to, body.impl_branch, body.code_mr_iid).await?)
}

/// POST .../docs/{doc}/draft/retry and .../draft/manual
pub async fn draft_retry(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid, doc)): Path<(i64, i64, String)>,
) -> ApiResult {
    let doc = parse_doc(&doc)?;
    let ctx = ctx(&state, &claims, project_id).await?;
    let flow = service::load_flow(&state, &ctx.project, iid).await?;
    service::retry_draft(&ctx, &flow, doc).await?;
    ok(json!({"started": true}))
}

pub async fn draft_manual(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid, doc)): Path<(i64, i64, String)>,
) -> ApiResult {
    let doc = parse_doc(&doc)?;
    let ctx = ctx(&state, &claims, project_id).await?;
    let flow = service::load_flow(&state, &ctx.project, iid).await?;
    service::reset_draft(&state, &flow, doc).await?;
    ok(json!({"manual": true}))
}

// ------------------------------------------------------------- websocket

/// POST .../ws-ticket — one-time ticket for the WebSocket (same mechanism
/// as the concurrency SSE ticket).
pub async fn ws_ticket(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, i64)>,
) -> ApiResult {
    let ctx = ctx(&state, &claims, project_id).await?;
    service::load_flow(&state, &ctx.project, iid).await?;
    let ticket = state.concurrency_manager.generate_ticket(ctx.user_id);
    ok(json!({"ticket": ticket}))
}

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub ticket: String,
}

/// GET .../ws?ticket= (no JWT middleware; ticket auth).
pub async fn ws(
    State(state): State<AppState>,
    Path((project_id, iid)): Path<(i64, i64)>,
    Query(q): Query<WsQuery>,
    upgrade: WebSocketUpgrade,
) -> Result<Response, WebPlatformError> {
    let user_id = state
        .concurrency_manager
        .validate_ticket(&q.ticket)
        .ok_or(WebPlatformError::Unauthorized)?;
    let user = crate::repository::UserRepository::find_by_id(&state.repo, user_id)
        .await?
        .ok_or(WebPlatformError::Unauthorized)?;
    let claims = Claims {
        sub: user_id.to_string(),
        username: user.username,
        role: user.role,
        iat: 0,
        exp: 0,
    };
    let ctx = ctx(&state, &claims, project_id).await?;
    let flow = service::load_flow(&state, &ctx.project, iid).await?;
    Ok(upgrade.on_upgrade(move |socket| ws_loop(socket, ctx, flow.id)))
}

async fn send(socket: &mut WebSocket, v: Value) -> bool {
    socket
        .send(Message::Text(v.to_string().into()))
        .await
        .is_ok()
}

async fn ws_loop(mut socket: WebSocket, ctx: Ctx, flow_id: i64) {
    let state = ctx.app.clone();
    let mut flow_rx = state.sdlc.flow_channel(flow_id).subscribe();
    // Subscribe to existing stage sessions; new ones are attached lazily.
    let mut agent_rx: HashMap<DocType, tokio::sync::broadcast::Receiver<Value>> = HashMap::new();
    let attach = |stage: DocType,
                  map: &mut HashMap<DocType, tokio::sync::broadcast::Receiver<Value>>|
     -> Option<Vec<Value>> {
        if map.contains_key(&stage) {
            return None;
        }
        let s = state.sdlc.agents.get(flow_id, stage)?;
        map.insert(stage, s.subscribe());
        Some(s.history())
    };
    for d in DocType::ALL {
        if let Some(h) = attach(d, &mut agent_rx) {
            if !send(
                &mut socket,
                json!({"type": "history", "stage": d, "events": h}),
            )
            .await
            {
                return;
            }
        }
    }
    let _ = send(&mut socket, json!({"type": "ready"})).await;

    // File watcher: notify on changes in the workspace.
    let (fs_tx, mut fs_rx) = tokio::sync::mpsc::channel::<Vec<String>>(64);
    let _watcher = match state.sdlc.store.get_flow_by_id(flow_id).await {
        Ok(Some(flow)) => match service::workspace_spec(&state, &ctx.project, &flow).await {
            Ok(spec) => start_watcher(
                state
                    .sdlc
                    .workspaces
                    .workspace_dir(spec.project_id, &spec.key),
                fs_tx,
            ),
            Err(_) => None,
        },
        _ => None,
    };

    loop {
        // Poll all agent receivers via a merged future.
        let agent_next = async {
            if agent_rx.is_empty() {
                std::future::pending::<(DocType, Option<Value>)>().await
            } else {
                let futs = agent_rx.iter_mut().map(|(stage, rx)| {
                    let stage = *stage;
                    Box::pin(async move {
                        match rx.recv().await {
                            Ok(v) => (stage, Some(v)),
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                (stage, Some(json!({"type": "lagged"})))
                            }
                            Err(_) => (stage, None),
                        }
                    })
                });
                futures::future::select_all(futs).await.0
            }
        };
        tokio::select! {
            msg = socket.recv() => {
                let Some(Ok(msg)) = msg else { break };
                let Message::Text(text) = msg else {
                    if matches!(msg, Message::Close(_)) { break; }
                    continue;
                };
                let Ok(cmd) = serde_json::from_str::<Value>(&text) else {
                    let _ = send(&mut socket, json!({"type": "error", "message": "invalid json"})).await;
                    continue;
                };
                let reply = handle_command(&ctx, flow_id, cmd).await;
                if let Some(stage) = reply.1 {
                    if let Some(h) = attach(stage, &mut agent_rx) {
                        let _ = send(&mut socket, json!({"type": "history", "stage": stage, "events": h})).await;
                    }
                }
                if let Some(r) = reply.0 {
                    if !send(&mut socket, r).await { break; }
                }
            }
            ev = flow_rx.recv() => {
                match ev {
                    Ok(v) => {
                        if v.get("type").and_then(|t| t.as_str()) == Some("agent_state") {
                            if let Some(stage) = v.get("stage").and_then(|s| s.as_str()).and_then(|s| s.parse::<DocType>().ok()) {
                                if let Some(h) = attach(stage, &mut agent_rx) {
                                    let _ = send(&mut socket, json!({"type": "history", "stage": stage, "events": h})).await;
                                }
                            }
                        }
                        if !send(&mut socket, v).await { break; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
            (stage, ev) = agent_next => {
                match ev {
                    Some(mut v) => {
                        v["stage"] = json!(stage);
                        if !send(&mut socket, json!({"type": "agent", "stage": stage, "event": v})).await { break; }
                    }
                    None => { agent_rx.remove(&stage); }
                }
            }
            paths = fs_rx.recv() => {
                if let Some(paths) = paths {
                    if !send(&mut socket, json!({"type": "fs_changed", "paths": paths})).await { break; }
                }
            }
        }
    }
}

/// Handle one browser command. Returns (reply, stage to attach).
async fn handle_command(ctx: &Ctx, flow_id: i64, cmd: Value) -> (Option<Value>, Option<DocType>) {
    let state = &ctx.app;
    let kind = cmd.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let stage = cmd
        .get("stage")
        .and_then(|s| s.as_str())
        .and_then(|s| s.parse::<DocType>().ok());
    let err = |m: String| Some(json!({"type": "error", "message": m}));
    match kind {
        "ping" => (Some(json!({"type": "pong"})), None),
        "send" => {
            let Some(stage) = stage else {
                return (err("缺少 stage".into()), None);
            };
            let text = cmd
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if text.is_empty() || text.len() > 20_000 {
                return (err("消息长度需为 1-20000".into()), None);
            }
            let text = with_context(&cmd, text);
            let flow = match state.sdlc.store.get_flow_by_id(flow_id).await {
                Ok(Some(f)) => f,
                _ => return (err("flow 不存在".into()), None),
            };
            // Session must exist before attaching.
            if let Err(e) = service::session_for(state, &ctx.project, &flow, stage).await {
                return (err(e.to_string()), None);
            }
            if state
                .sdlc
                .agents
                .get(flow_id, stage)
                .is_some_and(|s| s.is_running())
            {
                return (
                    err("AI 正在运行，请等待本轮结束或使用“追加说明”".into()),
                    Some(stage),
                );
            }
            let app = state.clone();
            let project = ctx.project.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    service::run_ai_turn(&app, &project, flow_id, stage, text, Mode::Interactive)
                        .await
                {
                    app.sdlc.notify(
                        flow_id,
                        json!({"type": "notice", "level": "error", "message": e.to_string()}),
                    );
                }
            });
            (Some(json!({"type": "accepted"})), Some(stage))
        }
        "steer" => {
            let Some(stage) = stage else {
                return (err("缺少 stage".into()), None);
            };
            let text = cmd
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            match state.sdlc.agents.get(flow_id, stage) {
                Some(s) => match s.steer(&text).await {
                    Ok(()) => (Some(json!({"type": "accepted"})), None),
                    Err(e) => (err(e.to_string()), None),
                },
                None => (err("AI 会话不存在".into()), None),
            }
        }
        "interrupt" => {
            if let Some(s) = stage.and_then(|st| state.sdlc.agents.get(flow_id, st)) {
                let _ = s.interrupt().await;
            }
            (Some(json!({"type": "accepted"})), None)
        }
        "answer" => {
            let Some(stage) = stage else {
                return (err("缺少 stage".into()), None);
            };
            let rid = cmd.get("requestId").and_then(|r| r.as_str()).unwrap_or("");
            let answer = cmd.get("answer").cloned().unwrap_or(Value::Null);
            match state.sdlc.agents.get(flow_id, stage) {
                Some(s) if s.answer(rid, answer) => (Some(json!({"type": "accepted"})), None),
                _ => (err("请求已失效".into()), None),
            }
        }
        "status" => {
            let sessions: Vec<Value> = state
                .sdlc
                .agents
                .flow_sessions(flow_id)
                .iter()
                .map(|s| json!({"stage": s.stage, "running": s.is_running(), "locked": s.locked_paths()}))
                .collect();
            (Some(json!({"type": "status", "sessions": sessions})), None)
        }
        other => (err(format!("unknown command {other}")), None),
    }
}

/// Attach `@file` references and the editor selection to the user message.
fn with_context(cmd: &Value, text: String) -> String {
    let mut out = text;
    if let Some(refs) = cmd.get("files").and_then(|f| f.as_array()) {
        let list: Vec<&str> = refs.iter().filter_map(|r| r.as_str()).take(20).collect();
        if !list.is_empty() {
            out.push_str("\n\nReferenced files (workspace-relative): ");
            out.push_str(&list.join(", "));
        }
    }
    if let Some(sel) = cmd.get("selection") {
        if let (Some(path), Some(text)) = (
            sel.get("path").and_then(|p| p.as_str()),
            sel.get("text").and_then(|t| t.as_str()),
        ) {
            let start = sel.get("startLine").and_then(|v| v.as_u64()).unwrap_or(0);
            let end = sel.get("endLine").and_then(|v| v.as_u64()).unwrap_or(start);
            let snippet: String = text.chars().take(8000).collect();
            out.push_str(&format!(
                "\n\nSelected text in `{path}` lines {start}-{end} (modify only this part unless asked otherwise):\n```\n{snippet}\n```"
            ));
        }
    }
    out
}

fn start_watcher(
    root: std::path::PathBuf,
    tx: tokio::sync::mpsc::Sender<Vec<String>>,
) -> Option<notify::RecommendedWatcher> {
    use notify::{RecursiveMode, Watcher};
    let (raw_tx, mut raw_rx) = tokio::sync::mpsc::unbounded_channel::<std::path::PathBuf>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            for p in ev.paths {
                let _ = raw_tx.send(p);
            }
        }
    })
    .ok()?;
    watcher.watch(&root, RecursiveMode::Recursive).ok()?;
    tokio::spawn(async move {
        loop {
            let Some(first) = raw_rx.recv().await else {
                break;
            };
            let mut batch = vec![first];
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            while let Ok(p) = raw_rx.try_recv() {
                batch.push(p);
            }
            let mut rels: Vec<String> = batch
                .iter()
                .filter_map(|p| p.strip_prefix(&root).ok())
                .map(|p| p.to_string_lossy().to_string())
                .filter(|p| !p.split('/').any(|seg| seg == ".git") && !p.ends_with(".symphony-tmp"))
                .collect();
            rels.sort();
            rels.dedup();
            if !rels.is_empty() && tx.send(rels).await.is_err() {
                break;
            }
        }
    });
    Some(watcher)
}

/// Writable check exposed for tests.
pub fn is_writable(writable: &[String], rel: &str) -> bool {
    WorkspaceService::is_writable(writable, rel)
}
