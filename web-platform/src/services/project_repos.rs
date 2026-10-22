//! Multi-repo workspace: resolution and spawn-time re-validation of `project_repos`.

use tracing::warn;

use crate::error::WebPlatformError;
use crate::handlers::token_validation::{
    fetch_gitlab_access_level, GITLAB_ACCESS_DEVELOPER, GITLAB_ACCESS_REPORTER,
};
use crate::models::{
    NewAuditLog, Project, ProjectRepo, ResolvedRepo, REPO_MODE_READ, REPO_STATUS_INVALID,
};
use crate::proxy::EffectiveProxyConfig;
use crate::repository::{
    ProjectMemberRepository, ProjectRepoRepository, ProjectRepository, SqliteRepository,
};

/// Audit `object_type` for `project_repos` rows.
pub const AUDIT_OBJECT_PROJECT_REPO: &str = "project_repo";

/// Host part (with port) of a `platform_host` such as `http://gitlab.example.com:8081/`.
pub fn platform_host_authority(platform_host: &str) -> String {
    let without_scheme = platform_host
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(platform_host);
    let authority = without_scheme.split('/').next().unwrap_or_default();
    let authority = authority.rsplit('@').next().unwrap_or_default();
    authority.to_lowercase()
}

/// Effective platform host of a project (same defaults as spawn).
pub fn effective_platform_host(project: &Project) -> String {
    project
        .platform_host
        .clone()
        .unwrap_or_else(|| match project.platform.as_str() {
            "github" => "https://github.com".to_string(),
            "gitea" => String::new(),
            _ => "https://gitlab.com".to_string(),
        })
}

/// Resolve one row to its slug and default branch (design 4.2 table).
/// `linked` is the referenced project when the row uses a linked source.
pub fn resolve_row(row: &ProjectRepo, linked: Option<&Project>) -> Option<ResolvedRepo> {
    let (slug, inherited_branch) = match (row.linked_project_id, linked) {
        (Some(_), Some(p)) => (
            format!("{}/{}", p.namespace, p.repo_name),
            Some(p.default_branch.clone()),
        ),
        (Some(_), None) => return None,
        (None, _) => (
            format!(
                "{}/{}",
                row.namespace.as_deref()?,
                row.repo_name.as_deref()?
            ),
            None,
        ),
    };
    let default_branch = row
        .default_branch
        .clone()
        .filter(|b| !b.trim().is_empty())
        .or(inherited_branch)?;
    Some(ResolvedRepo {
        alias: row.alias.clone(),
        slug,
        default_branch,
        mode: row.mode.clone(),
        checkout: row.checkout.clone(),
        merge_after: row.merge_after.clone(),
    })
}

/// Resolve active rows for display (workflow preview). Never mutates the DB;
/// invalid or unresolvable rows are skipped.
pub async fn resolve_active_repos(
    repo: &SqliteRepository,
    project: &Project,
) -> Result<Vec<ResolvedRepo>, WebPlatformError> {
    let rows = repo.list_project_repos(project.id).await?;
    let mut resolved = Vec::with_capacity(rows.len());
    for row in rows.iter().filter(|r| !r.is_invalid()) {
        let linked = match row.linked_project_id {
            Some(id) => repo.get_project(id).await?,
            None => None,
        };
        if let Some(r) = resolve_row(row, linked.as_ref()) {
            resolved.push(r);
        }
    }
    Ok(resolved)
}

/// Optional network check performed at spawn time.
pub struct SpawnAccessCheck<'a> {
    /// Token of the main project's `created_by` (the one used by the agent).
    pub token: &'a str,
    pub proxy_config: &'a EffectiveProxyConfig,
}

async fn invalidate(
    repo: &SqliteRepository,
    row: &ProjectRepo,
    reason: String,
) -> Result<WebPlatformError, WebPlatformError> {
    repo.set_project_repo_status(row.id, REPO_STATUS_INVALID, Some(&reason))
        .await?;
    let _ = repo
        .insert_audit_log(&NewAuditLog {
            actor_user_id: None,
            project_id: Some(row.project_id),
            object_type: AUDIT_OBJECT_PROJECT_REPO.to_string(),
            object_id: Some(row.alias.clone()),
            action: "invalidate".to_string(),
            before_json: serde_json::to_string(row).ok(),
            after_json: Some(serde_json::json!({ "status_reason": reason }).to_string()),
        })
        .await;
    Ok(refuse_error(&row.alias, &reason))
}

fn refuse_error(alias: &str, reason: &str) -> WebPlatformError {
    WebPlatformError::Conflict(format!(
        "关联仓库 {} 已失效：{}。请在项目设置中修复关联仓库后再启动服务",
        alias, reason
    ))
}

/// Re-validate all related repositories before a service start (design D7) and
/// resolve them for WORKFLOW.md rendering.
///
/// - any `invalid` row refuses the start;
/// - the saver (`created_by` of the row) must still be a member of the main project;
/// - for linked rows, the main project's `created_by` must still be a member of the
///   linked project;
/// - `write` rows whose `write_granted_by` differs from the main project's
///   `created_by` are downgraded to `read` (warning only);
/// - when `access` is given (GitLab), the token's access level is checked
///   best-effort: an insufficient level invalidates the row, network errors only warn.
pub async fn validate_repos_for_spawn(
    repo: &SqliteRepository,
    project: &Project,
    access: Option<SpawnAccessCheck<'_>>,
) -> Result<Vec<ResolvedRepo>, WebPlatformError> {
    let mut rows = repo.list_project_repos(project.id).await?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    if let Some(row) = rows.iter().find(|r| r.is_invalid()) {
        return Err(refuse_error(
            &row.alias,
            row.status_reason.as_deref().unwrap_or("未知原因"),
        ));
    }

    let owner_id = project.created_by;
    let platform_host = effective_platform_host(project);
    let mut resolved = Vec::with_capacity(rows.len());

    for row in rows.iter_mut() {
        if !repo.is_member(project.id, row.created_by).await? {
            let reason = format!("保存人（用户 {}）已不是本项目成员", row.created_by);
            return Err(invalidate(repo, row, reason).await?);
        }

        let linked = match row.linked_project_id {
            Some(linked_id) => {
                let Some(linked) = repo.get_project(linked_id).await? else {
                    let reason = format!("被引用的项目 {} 不存在", linked_id);
                    return Err(invalidate(repo, row, reason).await?);
                };
                let owner_is_member = match owner_id {
                    Some(owner) => repo.is_member(linked_id, owner).await?,
                    None => false,
                };
                if !owner_is_member {
                    let reason = format!("本项目创建人已不是被引用项目 {} 的成员", linked.name);
                    return Err(invalidate(repo, row, reason).await?);
                }
                Some(linked)
            }
            None => None,
        };

        if row.is_write() && row.write_granted_by != owner_id {
            warn!(
                project_id = project.id,
                alias = %row.alias,
                "write grant does not match project created_by; downgrading to read"
            );
            repo.downgrade_project_repo_to_read(row.id).await?;
            let _ = repo
                .insert_audit_log(&NewAuditLog {
                    actor_user_id: None,
                    project_id: Some(project.id),
                    object_type: AUDIT_OBJECT_PROJECT_REPO.to_string(),
                    object_id: Some(row.alias.clone()),
                    action: "downgrade_write".to_string(),
                    before_json: serde_json::to_string(&*row).ok(),
                    after_json: Some(
                        serde_json::json!({ "mode": REPO_MODE_READ, "reason": "write_granted_by != created_by" })
                            .to_string(),
                    ),
                })
                .await;
            row.mode = REPO_MODE_READ.to_string();
            row.write_granted_by = None;
            row.write_granted_at = None;
        }

        let Some(resolved_row) = resolve_row(row, linked.as_ref()) else {
            let reason = "无法解析仓库坐标或默认分支".to_string();
            return Err(invalidate(repo, row, reason).await?);
        };

        if let Some(ref access) = access {
            let required = if row.is_write() {
                GITLAB_ACCESS_DEVELOPER
            } else {
                GITLAB_ACCESS_REPORTER
            };
            match fetch_gitlab_access_level(
                &platform_host,
                access.token,
                &resolved_row.slug,
                access.proxy_config,
            )
            .await
            {
                Ok(level) if level < required => {
                    let reason = format!(
                        "项目创建人对 {} 的访问级别不足（当前 {}，需要 {}）",
                        resolved_row.slug, level, required
                    );
                    return Err(invalidate(repo, row, reason).await?);
                }
                Ok(_) => {}
                Err(e) => warn!(
                    project_id = project.id,
                    alias = %row.alias,
                    error = %e,
                    "skipping access level check for related repo"
                ),
            }
        }

        resolved.push(resolved_row);
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(linked: Option<i64>, ns: Option<&str>, branch: Option<&str>) -> ProjectRepo {
        ProjectRepo {
            id: 1,
            project_id: 1,
            alias: "backend".to_string(),
            linked_project_id: linked,
            namespace: ns.map(str::to_string),
            repo_name: ns.map(|_| "jxy".to_string()),
            default_branch: branch.map(str::to_string),
            mode: "read".to_string(),
            checkout: "partial".to_string(),
            merge_after: vec![],
            status: "active".to_string(),
            status_reason: None,
            write_granted_by: None,
            write_granted_at: None,
            created_by: 1,
            created_at: String::new(),
        }
    }

    #[test]
    fn authority_strips_scheme_path_and_userinfo() {
        assert_eq!(
            platform_host_authority("http://GitLab.example.com:8081/"),
            "gitlab.example.com:8081"
        );
        assert_eq!(platform_host_authority("gitlab.com"), "gitlab.com");
        assert_eq!(
            platform_host_authority("https://u:p@gitlab.com/x"),
            "gitlab.com"
        );
    }

    #[test]
    fn raw_row_uses_its_own_branch() {
        let r = resolve_row(&row(None, Some("JstAi"), Some("pre")), None).unwrap();
        assert_eq!(r.slug, "JstAi/jxy");
        assert_eq!(r.default_branch, "pre");
    }

    #[test]
    fn raw_row_without_branch_is_unresolvable() {
        assert!(resolve_row(&row(None, Some("JstAi"), None), None).is_none());
    }
}
