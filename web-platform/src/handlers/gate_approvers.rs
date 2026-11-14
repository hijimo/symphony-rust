//! `GET/PUT /api/projects/{id}/gate-approvers` (P2 design §4 B6).

use std::collections::{BTreeMap, HashSet};

use axum::extract::{Path, State};
use axum::Json;

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::middleware::project_access::{require_project_member, require_project_owner};
use crate::models::ResponseData;
use crate::repository::ProjectMemberRepository;
use crate::sdlc::model::Gate;
use crate::sdlc::store::GateApprover;
use crate::AppState;

type Lists = BTreeMap<String, Vec<GateApprover>>;

/// GET — approver lists per gate; an empty list means every member.
pub async fn get_gate_approvers(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<Lists>>, WebPlatformError> {
    require_project_member(&claims, project_id, &state.repo).await?;
    let lists = state.sdlc.store.gate_approvers(project_id).await?;
    Ok(Json(ResponseData::success(lists)))
}

/// PUT — replace all lists (owners and admins only). Body
/// `{"G1": [userId], "G2": [], "G3": []}`; omitted gates become empty.
pub async fn put_gate_approvers(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(body): Json<BTreeMap<String, Vec<i64>>>,
) -> Result<Json<ResponseData<Lists>>, WebPlatformError> {
    require_project_owner(&claims, project_id, &state.repo).await?;
    let members: HashSet<i64> = state
        .repo
        .list_members(project_id)
        .await?
        .into_iter()
        .map(|m| m.user_id)
        .collect();
    let mut lists = Vec::new();
    for (key, users) in body {
        let gate = match key.as_str() {
            "G1" => Gate::G1,
            "G2" => Gate::G2,
            "G3" => Gate::G3,
            other => {
                return Err(WebPlatformError::BadRequest(format!("未知闸门：{other}")));
            }
        };
        if let Some(u) = users.iter().find(|u| !members.contains(u)) {
            return Err(WebPlatformError::BadRequest(format!(
                "用户 {u} 不是项目成员"
            )));
        }
        lists.push((gate, users));
    }
    state
        .sdlc
        .store
        .set_gate_approvers(project_id, lists)
        .await?;
    let lists = state.sdlc.store.gate_approvers(project_id).await?;
    Ok(Json(ResponseData::success(lists)))
}
