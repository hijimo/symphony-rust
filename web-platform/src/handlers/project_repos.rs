//! Related repositories of a project (multi-repo workspace, design 4.2).
//!
//! - `GET  /api/projects/{id}/repos`             member
//! - `PUT  /api/projects/{id}/repos`             owner, full replace
//! - `GET  /api/projects/{id}/repos/candidates`  owner; projects the caller owns

use std::collections::{HashMap, HashSet};

use axum::{
    extract::{Path, State},
    Json,
};
use regex::Regex;

use crate::auth::jwt::Claims;
use crate::crypto;
use crate::error::WebPlatformError;
use crate::git_url::parse_git_url;
use crate::handlers::network_proxy::load_effective_proxy_config;
use crate::handlers::token_validation::{fetch_gitlab_access_level, GITLAB_ACCESS_REPORTER};
use crate::middleware::project_access::{require_project_member, require_project_owner};
use crate::models::{
    NewAuditLog, NewProjectRepo, Project, ProjectRepo, ProjectRepoCandidate, ProjectRepoInput,
    ProjectRepoView, ReplaceProjectReposRequest, ResponseData, REPO_MODE_READ, REPO_MODE_WRITE,
    REPO_STATUS_ACTIVE,
};
use crate::repository::{
    ProjectMemberRepository, ProjectRepoRepository, ProjectRepository, UserConfigRepository,
};
use crate::services::project_repos::{
    effective_platform_host, platform_host_authority, AUDIT_OBJECT_PROJECT_REPO,
};
use crate::templates::PRIMARY_REPO_ALIAS;
use crate::AppState;

/// Aliases that cannot be used for related repositories (directory names).
/// Must stay a superset of rust-platform `config/repos.rs` `RESERVED_ALIASES`.
pub const RESERVED_ALIASES: &[&str] = &[
    PRIMARY_REPO_ALIAS,
    ".symphony",
    "symphony",
    "tmp",
    "quarantine",
    "workspace",
];
const MAX_REPOS: usize = 20;
const MAX_ALIAS_LEN: usize = 50;
const MAX_BRANCH_LEN: usize = 100;

fn current_user_id(claims: &Claims) -> Result<i64, WebPlatformError> {
    claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))
}

fn bad(msg: impl Into<String>) -> WebPlatformError {
    WebPlatformError::BadRequest(msg.into())
}

/// Validate an alias: `^[a-z0-9-]+$`, bounded length, not reserved.
pub fn validate_alias(alias: &str) -> Result<(), WebPlatformError> {
    let re = Regex::new(r"^[a-z0-9-]+$").unwrap();
    if alias.is_empty() || alias.len() > MAX_ALIAS_LEN || !re.is_match(alias) {
        return Err(bad(format!(
            "alias '{}' 只能包含小写字母、数字和 -，长度 1-{}",
            alias, MAX_ALIAS_LEN
        )));
    }
    if RESERVED_ALIASES.contains(&alias) {
        return Err(bad(format!("alias '{}' 是保留名", alias)));
    }
    if alias.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return Err(bad(format!("alias '{}' 不能只由数字和 - 组成", alias)));
    }
    Ok(())
}

/// Git ref check identical to rust-platform `config/repos.rs::is_safe_ref`,
/// so anything accepted here also parses at worker start.
pub fn is_safe_ref(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('-')
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.ends_with(".lock")
        && !name.contains("..")
        && !name.contains("@{")
        && !name.contains("//")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

/// Slug check identical to rust-platform `validate_repos`.
pub fn is_safe_slug(slug: &str) -> bool {
    slug.split('/').filter(|p| !p.is_empty()).count() >= 2
        && !slug.contains("..")
        && !slug.contains("://")
        && slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

/// Mirror of rust-platform `clones_into_workspace_root`: `git clone ... .`
/// in `after_create` would collide with `<workspace>/<alias>/` checkouts.
pub fn clones_into_workspace_root(script: &str) -> bool {
    script.lines().any(|line| {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(pos) = tokens
            .windows(2)
            .position(|w| w[0] == "git" && w[1] == "clone")
        else {
            return false;
        };
        let args: Vec<&str> = tokens[pos + 2..]
            .iter()
            .copied()
            .take_while(|t| !matches!(*t, "&&" | "||" | ";" | "|"))
            .collect();
        matches!(args.last(), Some(&".") | Some(&"./"))
    })
}

/// `merge_after` must reference known aliases (or `primary`), not itself, and be acyclic.
pub fn validate_merge_after(rows: &[(String, Vec<String>)]) -> Result<(), WebPlatformError> {
    let deduped: Vec<(String, Vec<String>)> = rows
        .iter()
        .map(|(a, deps)| {
            let mut seen = HashSet::new();
            let d = deps
                .iter()
                .filter(|d| seen.insert(d.as_str()))
                .cloned()
                .collect();
            (a.clone(), d)
        })
        .collect();
    let rows = &deduped[..];
    let aliases: HashSet<&str> = rows.iter().map(|(a, _)| a.as_str()).collect();
    for (alias, deps) in rows {
        for dep in deps {
            if dep == alias {
                return Err(bad(format!("{} 的 merge_after 不能引用自身", alias)));
            }
            if dep != PRIMARY_REPO_ALIAS && !aliases.contains(dep.as_str()) {
                return Err(bad(format!(
                    "{} 的 merge_after 引用了不存在的 alias '{}'",
                    alias, dep
                )));
            }
        }
    }
    // Kahn's algorithm over secondary repos (primary has no configurable deps here).
    let mut indegree: HashMap<&str, usize> = rows.iter().map(|(a, _)| (a.as_str(), 0)).collect();
    for (alias, deps) in rows {
        let n = deps.iter().filter(|d| aliases.contains(d.as_str())).count();
        *indegree.get_mut(alias.as_str()).unwrap() = n;
    }
    let mut ready: Vec<&str> = indegree
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(a, _)| *a)
        .collect();
    let mut visited = 0;
    while let Some(done) = ready.pop() {
        visited += 1;
        for (alias, deps) in rows {
            if deps.iter().any(|d| d == done) {
                let n = indegree.get_mut(alias.as_str()).unwrap();
                *n -= 1;
                if *n == 0 {
                    ready.push(alias.as_str());
                }
            }
        }
    }
    if visited != rows.len() {
        return Err(bad("merge_after 存在循环依赖"));
    }
    Ok(())
}

/// A request row after local (non-permission) validation.
struct Candidate {
    alias: String,
    linked: Option<Project>,
    namespace: Option<String>,
    repo_name: Option<String>,
    default_branch: Option<String>,
    mode: String,
    checkout: String,
    merge_after: Vec<String>,
}

impl Candidate {
    fn coordinates(&self) -> (Option<i64>, Option<String>, Option<String>) {
        (
            self.linked.as_ref().map(|p| p.id),
            self.namespace.clone(),
            self.repo_name.clone(),
        )
    }

    /// Repository identity: lowercase `namespace/repo_name` on the main instance.
    fn identity(&self) -> String {
        match &self.linked {
            Some(p) => format!("{}/{}", p.namespace, p.repo_name),
            None => format!(
                "{}/{}",
                self.namespace.as_deref().unwrap_or_default(),
                self.repo_name.as_deref().unwrap_or_default()
            ),
        }
        .to_lowercase()
    }

    fn slug(&self) -> String {
        match &self.linked {
            Some(p) => format!("{}/{}", p.namespace, p.repo_name),
            None => format!(
                "{}/{}",
                self.namespace.as_deref().unwrap_or_default(),
                self.repo_name.as_deref().unwrap_or_default()
            ),
        }
    }

    /// Whether this row is identical to the stored one (no re-validation needed).
    fn same_as(&self, old: &ProjectRepo) -> bool {
        self.coordinates() == old.coordinates()
            && self.default_branch == old.default_branch
            && self.mode == old.mode
            && self.checkout == old.checkout
            && self.merge_after == old.merge_after
    }
}

async fn parse_input(
    state: &AppState,
    main: &Project,
    input: ProjectRepoInput,
) -> Result<Candidate, WebPlatformError> {
    let alias = input.alias.trim().to_string();
    validate_alias(&alias)?;

    let mode = input.mode.unwrap_or_else(|| REPO_MODE_READ.to_string());
    if mode != REPO_MODE_READ && mode != REPO_MODE_WRITE {
        return Err(bad(format!("{}: mode 必须是 read 或 write", alias)));
    }
    let checkout = input.checkout.unwrap_or_else(|| "partial".to_string());
    if checkout != "partial" && checkout != "full" {
        return Err(bad(format!("{}: checkout 必须是 full 或 partial", alias)));
    }
    let default_branch = input
        .default_branch
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty());
    if default_branch
        .as_ref()
        .is_some_and(|b| b.len() > MAX_BRANCH_LEN)
    {
        return Err(bad(format!("{}: default_branch 过长", alias)));
    }
    if default_branch.as_ref().is_some_and(|b| !is_safe_ref(b)) {
        return Err(bad(format!("{}: default_branch 不是合法的分支名", alias)));
    }
    let merge_after = input.merge_after.unwrap_or_default();

    let git_url = input
        .git_url
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty());
    let has_coords = input.namespace.is_some() || input.repo_name.is_some();
    let sources = [
        input.linked_project_id.is_some(),
        git_url.is_some(),
        has_coords,
    ]
    .iter()
    .filter(|s| **s)
    .count();
    if sources != 1 {
        return Err(bad(format!(
            "{}: 必须且只能指定一种来源（linked_project_id 或 git_url）",
            alias
        )));
    }

    let main_authority = platform_host_authority(&effective_platform_host(main));

    if let Some(linked_id) = input.linked_project_id {
        if linked_id == main.id {
            return Err(bad(format!("{}: 不能关联项目自身", alias)));
        }
        let linked = state
            .repo
            .get_project(linked_id)
            .await?
            .ok_or_else(|| bad(format!("{}: 被引用的项目 {} 不存在", alias, linked_id)))?;
        if linked.platform != main.platform
            || platform_host_authority(&effective_platform_host(&linked)) != main_authority
        {
            return Err(bad(format!(
                "{}: 被引用项目必须与主项目在同一个平台实例上",
                alias
            )));
        }
        return Ok(Candidate {
            alias,
            linked: Some(linked),
            namespace: None,
            repo_name: None,
            default_branch,
            mode,
            checkout,
            merge_after,
        });
    }

    let (namespace, repo_name) = if let Some(url) = git_url {
        let parsed =
            parse_git_url(&url).map_err(|e| bad(format!("{}: 仓库地址无效：{}", alias, e)))?;
        if parsed.host.to_lowercase() != main_authority {
            return Err(bad(format!(
                "{}: 仓库必须与主项目在同一个平台实例上（{}）",
                alias, main_authority
            )));
        }
        (parsed.namespace, parsed.repo_name)
    } else {
        let namespace = input.namespace.unwrap_or_default().trim().to_string();
        let repo_name = input.repo_name.unwrap_or_default().trim().to_string();
        if namespace.is_empty() || repo_name.is_empty() {
            return Err(bad(format!("{}: namespace 和 repo_name 都必须填写", alias)));
        }
        (namespace, repo_name)
    };
    if default_branch.is_none() {
        return Err(bad(format!(
            "{}: 直接填写的仓库必须指定 default_branch",
            alias
        )));
    }
    if !is_safe_slug(&format!("{namespace}/{repo_name}")) {
        return Err(bad(format!(
            "{}: 仓库路径 {}/{} 不合法",
            alias, namespace, repo_name
        )));
    }
    Ok(Candidate {
        alias,
        linked: None,
        namespace: Some(namespace),
        repo_name: Some(repo_name),
        default_branch,
        mode,
        checkout,
        merge_after,
    })
}

/// Saver must have their own GitLab token with >= Reporter on a raw repository.
async fn check_raw_access(
    state: &AppState,
    main: &Project,
    saver_id: i64,
    candidate: &Candidate,
) -> Result<(), WebPlatformError> {
    if main.platform != "gitlab" {
        return Err(bad(format!(
            "{}: 直接填写仓库目前只支持 GitLab 项目，请改为引用 Symphony 项目",
            candidate.alias
        )));
    }
    let platform_host = effective_platform_host(main);
    let config = state.repo.get_config(saver_id).await?;
    let config = config
        .filter(|c| c.gitlab_token.is_some())
        .ok_or_else(|| bad("请先在个人设置中配置 GitLab Token，再添加直接填写的关联仓库"))?;
    if let Some(ref host) = config.gitlab_host {
        if !host.trim().is_empty()
            && platform_host_authority(host) != platform_host_authority(&platform_host)
        {
            return Err(bad(
                "个人设置中的 GitLab 地址与本项目的平台实例不一致，请先修正个人设置",
            ));
        }
    }
    let token = crypto::decrypt(
        config.gitlab_token.as_deref().unwrap(),
        &state.encryption_key,
    )?;
    let proxy_config = load_effective_proxy_config(&state.repo, &state.encryption_key).await?;
    let slug = candidate.slug();
    let level = fetch_gitlab_access_level(&platform_host, &token, &slug, &proxy_config)
        .await
        .map_err(|e| {
            WebPlatformError::ExternalService(format!("校验 {} 的访问权限失败：{}", slug, e))
        })?;
    if level < GITLAB_ACCESS_REPORTER {
        return Err(bad(format!(
            "{}: 你的 GitLab Token 对 {} 没有 Reporter 及以上权限",
            candidate.alias, slug
        )));
    }
    Ok(())
}

fn audit(
    actor: i64,
    project_id: i64,
    alias: &str,
    action: &str,
    before: Option<&ProjectRepo>,
    after: Option<&NewProjectRepo>,
    via_admin: bool,
) -> NewAuditLog {
    let after_json = after.map(|a| {
        let mut v = serde_json::to_value(a).unwrap_or_default();
        if via_admin {
            v["via_admin"] = serde_json::Value::Bool(true);
        }
        v.to_string()
    });
    let after_json = match (after_json, via_admin) {
        (None, true) => Some(serde_json::json!({ "via_admin": true }).to_string()),
        (a, _) => a,
    };
    NewAuditLog {
        actor_user_id: Some(actor),
        project_id: Some(project_id),
        object_type: AUDIT_OBJECT_PROJECT_REPO.to_string(),
        object_id: Some(alias.to_string()),
        action: action.to_string(),
        before_json: before.and_then(|b| serde_json::to_string(b).ok()),
        after_json,
    }
}

async fn to_views(
    state: &AppState,
    rows: Vec<ProjectRepo>,
) -> Result<Vec<ProjectRepoView>, WebPlatformError> {
    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        let linked = match row.linked_project_id {
            Some(id) => state.repo.get_project(id).await?,
            None => None,
        };
        views.push(ProjectRepoView {
            id: row.id,
            alias: row.alias,
            source: if row.linked_project_id.is_some() {
                "linked".to_string()
            } else {
                "raw".to_string()
            },
            linked_project_id: row.linked_project_id,
            linked_project_name: linked.as_ref().map(|p| p.name.clone()),
            namespace: row
                .namespace
                .or_else(|| linked.as_ref().map(|p| p.namespace.clone())),
            repo_name: row
                .repo_name
                .or_else(|| linked.as_ref().map(|p| p.repo_name.clone())),
            default_branch: row.default_branch,
            inherited_default_branch: linked.as_ref().map(|p| p.default_branch.clone()),
            mode: row.mode,
            checkout: row.checkout,
            merge_after: row.merge_after,
            status: row.status,
            status_reason: row.status_reason,
            write_granted_by: row.write_granted_by,
            write_granted_at: row.write_granted_at,
            created_by: row.created_by,
            created_at: row.created_at,
        });
    }
    Ok(views)
}

/// GET /api/projects/{id}/repos
pub async fn list_repos(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<Vec<ProjectRepoView>>>, WebPlatformError> {
    require_project_member(&claims, project_id, &state.repo).await?;
    let rows = state.repo.list_project_repos(project_id).await?;
    Ok(Json(ResponseData::success(to_views(&state, rows).await?)))
}

/// PUT /api/projects/{id}/repos — full replace, compared with stored rows by alias.
pub async fn replace_repos(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(req): Json<ReplaceProjectReposRequest>,
) -> Result<Json<ResponseData<Vec<ProjectRepoView>>>, WebPlatformError> {
    let my_role = require_project_owner(&claims, project_id, &state.repo).await?;
    let saver_id = current_user_id(&claims)?;
    let is_admin = claims.role == "admin";
    let via_admin = my_role == "admin";

    let main = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    if req.repos.len() > MAX_REPOS {
        return Err(bad(format!("关联仓库最多 {} 个", MAX_REPOS)));
    }
    if !req.repos.is_empty()
        && main
            .hooks_after_create
            .as_deref()
            .is_some_and(clones_into_workspace_root)
    {
        return Err(bad(
            "项目的 after_create hook 会 clone 到工作区根目录，与关联仓库冲突；\
             启用关联仓库后仓库由 Symphony 检出到 <workspace>/<alias>/，请先移除该 clone 命令",
        ));
    }

    // 1. Local validation.
    let mut candidates = Vec::with_capacity(req.repos.len());
    for input in req.repos {
        candidates.push(parse_input(&state, &main, input).await?);
    }

    let mut aliases = HashSet::new();
    for c in &candidates {
        if !aliases.insert(c.alias.clone()) {
            return Err(bad(format!("alias '{}' 重复", c.alias)));
        }
    }
    let merge_graph: Vec<(String, Vec<String>)> = candidates
        .iter()
        .map(|c| (c.alias.clone(), c.merge_after.clone()))
        .collect();
    validate_merge_after(&merge_graph)?;

    // Dedupe by repository coordinates (both sources and vs the main project).
    let main_identity = format!("{}/{}", main.namespace, main.repo_name).to_lowercase();
    let mut identities: HashMap<String, String> = HashMap::new();
    for c in &candidates {
        let identity = c.identity();
        if identity == main_identity {
            return Err(bad(format!("{}: 不能把主仓库作为关联仓库", c.alias)));
        }
        if let Some(other) = identities.insert(identity, c.alias.clone()) {
            return Err(bad(format!(
                "{} 与 {} 指向同一个仓库 {}",
                other,
                c.alias,
                c.slug()
            )));
        }
    }

    // 2. Permission checks, compared with stored rows by alias.
    let old_rows = state.repo.list_project_repos(project_id).await?;
    let old_by_alias: HashMap<&str, &ProjectRepo> =
        old_rows.iter().map(|r| (r.alias.as_str(), r)).collect();
    let is_created_by = main.created_by == Some(saver_id);
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let mut new_rows = Vec::with_capacity(candidates.len());
    let mut logs = Vec::new();

    for c in &candidates {
        let old = old_by_alias.get(c.alias.as_str()).copied();

        // Unchanged active rows are kept as-is (no re-validation).
        if let Some(old) = old.filter(|o| !o.is_invalid() && c.same_as(o)) {
            new_rows.push(NewProjectRepo {
                alias: old.alias.clone(),
                linked_project_id: old.linked_project_id,
                namespace: old.namespace.clone(),
                repo_name: old.repo_name.clone(),
                default_branch: old.default_branch.clone(),
                mode: old.mode.clone(),
                checkout: old.checkout.clone(),
                merge_after: old.merge_after.clone(),
                status: old.status.clone(),
                status_reason: old.status_reason.clone(),
                write_granted_by: old.write_granted_by,
                write_granted_at: old.write_granted_at.clone(),
                created_by: old.created_by,
                created_at: Some(old.created_at.clone()),
            });
            continue;
        }

        let coords_changed = old.is_some_and(|o| o.coordinates() != c.coordinates());
        // An invalid row is treated like a brand-new one: every permission
        // check runs again before it can become active.
        let revalidate = old.is_some_and(|o| o.is_invalid());
        let source_changed = old.is_none() || coords_changed || revalidate;

        if source_changed {
            match &c.linked {
                Some(linked) => {
                    if !is_admin {
                        let role = state.repo.get_member_role(linked.id, saver_id).await?;
                        if role.as_deref() != Some("owner") {
                            return Err(bad(format!(
                                "{}: 你必须同时是被引用项目 {} 的 owner",
                                c.alias, linked.name
                            )));
                        }
                    }
                }
                None => check_raw_access(&state, &main, saver_id, c).await?,
            }
        }

        let mut mode = c.mode.clone();
        let (mut granted_by, mut granted_at) = (None, None);
        let mut downgraded = false;
        if mode == REPO_MODE_WRITE {
            let kept_grant = old.filter(|o| o.is_write() && !coords_changed);
            if let Some(o) = kept_grant {
                // Same coordinates, already granted: keep the original grant.
                granted_by = o.write_granted_by;
                granted_at = o.write_granted_at.clone();
            } else if is_created_by {
                granted_by = Some(saver_id);
                granted_at = Some(now.clone());
            } else if old.is_some_and(|o| o.is_write()) && coords_changed {
                // Changing the coordinates of a write row re-grants write access;
                // only created_by may do so. Others: downgrade to read.
                mode = REPO_MODE_READ.to_string();
                downgraded = true;
            } else {
                return Err(WebPlatformError::ForbiddenWithReason(format!(
                    "{}: 只有项目创建人可以把关联仓库设置为可写（mode: write）",
                    c.alias
                )));
            }
        }

        let row = NewProjectRepo {
            alias: c.alias.clone(),
            linked_project_id: c.linked.as_ref().map(|p| p.id),
            namespace: c.namespace.clone(),
            repo_name: c.repo_name.clone(),
            default_branch: c.default_branch.clone(),
            mode,
            checkout: c.checkout.clone(),
            merge_after: c.merge_after.clone(),
            status: REPO_STATUS_ACTIVE.to_string(),
            status_reason: None,
            write_granted_by: granted_by,
            write_granted_at: granted_at,
            // The saver of record only changes when the source was
            // (re-)validated for this saver; editing mode/checkout/branch of
            // an existing row keeps the original saver (and their checks).
            created_by: match old {
                Some(o) if !source_changed => o.created_by,
                _ => saver_id,
            },
            created_at: if source_changed {
                None
            } else {
                old.map(|o| o.created_at.clone())
            },
        };
        let action = if old.is_some() { "update" } else { "create" };
        logs.push(audit(
            saver_id,
            project_id,
            &c.alias,
            action,
            old,
            Some(&row),
            via_admin,
        ));
        if downgraded {
            logs.push(audit(
                saver_id,
                project_id,
                &c.alias,
                "downgrade_write",
                old,
                Some(&row),
                via_admin,
            ));
        }
        new_rows.push(row);
    }

    for old in &old_rows {
        if !aliases.contains(&old.alias) {
            logs.push(audit(
                saver_id,
                project_id,
                &old.alias,
                "delete",
                Some(old),
                None,
                via_admin,
            ));
        }
    }

    let saved = state
        .repo
        .replace_project_repos(project_id, new_rows, logs)
        .await?;
    Ok(Json(ResponseData::success(to_views(&state, saved).await?)))
}

/// GET /api/projects/{id}/repos/candidates — projects the caller owns (admins too),
/// excluding this project and already-linked ones.
pub async fn list_candidates(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<Vec<ProjectRepoCandidate>>>, WebPlatformError> {
    require_project_owner(&claims, project_id, &state.repo).await?;
    let user_id = current_user_id(&claims)?;
    let linked: HashSet<i64> = state
        .repo
        .list_project_repos(project_id)
        .await?
        .into_iter()
        .filter_map(|r| r.linked_project_id)
        .collect();
    let candidates = state
        .repo
        .list_owned_projects(user_id)
        .await?
        .into_iter()
        .filter(|p| p.id != project_id && !linked.contains(&p.id))
        .collect();
    Ok(Json(ResponseData::success(candidates)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_rules() {
        assert!(validate_alias("backend").is_ok());
        assert!(validate_alias("api-2").is_ok());
        for bad_alias in ["", "Backend", "a_b", ".symphony", "a/b", "primary", "tmp"] {
            assert!(validate_alias(bad_alias).is_err(), "{bad_alias}");
        }
        assert!(validate_alias(&"a".repeat(51)).is_err());
    }

    fn g(rows: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        rows.iter()
            .map(|(a, d)| (a.to_string(), d.iter().map(|s| s.to_string()).collect()))
            .collect()
    }

    #[test]
    fn merge_after_rules() {
        assert!(validate_merge_after(&g(&[("a", &["primary"]), ("b", &["a"])])).is_ok());
        assert!(validate_merge_after(&g(&[("a", &["a"])])).is_err());
        assert!(validate_merge_after(&g(&[("a", &["x"])])).is_err());
        assert!(validate_merge_after(&g(&[("a", &["b"]), ("b", &["a"])])).is_err());
        assert!(validate_merge_after(&g(&[("a", &["b"]), ("b", &["c"]), ("c", &["a"])])).is_err());
    }
}
