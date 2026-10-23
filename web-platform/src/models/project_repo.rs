use serde::{Deserialize, Serialize};

/// Repository mode: `read` (reference only) or `write` (agent may push / open MRs).
pub const REPO_MODE_READ: &str = "read";
pub const REPO_MODE_WRITE: &str = "write";
/// Row status.
pub const REPO_STATUS_ACTIVE: &str = "active";
pub const REPO_STATUS_INVALID: &str = "invalid";

/// A related (secondary) repository of a project, as stored in `project_repos`.
///
/// Exactly one source is set: `linked_project_id` (reference an existing Symphony
/// project) or `namespace` + `repo_name` (raw repository on the same platform instance).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectRepo {
    pub id: i64,
    pub project_id: i64,
    pub alias: String,
    pub linked_project_id: Option<i64>,
    pub namespace: Option<String>,
    pub repo_name: Option<String>,
    pub default_branch: Option<String>,
    pub mode: String,
    pub checkout: String,
    pub merge_after: Vec<String>,
    pub status: String,
    pub status_reason: Option<String>,
    pub write_granted_by: Option<i64>,
    pub write_granted_at: Option<String>,
    pub created_by: i64,
    pub created_at: String,
}

impl ProjectRepo {
    pub fn is_write(&self) -> bool {
        self.mode == REPO_MODE_WRITE
    }

    pub fn is_invalid(&self) -> bool {
        self.status == REPO_STATUS_INVALID
    }

    /// Repository coordinates; write permission is bound to these.
    pub fn coordinates(&self) -> (Option<i64>, Option<String>, Option<String>) {
        (
            self.linked_project_id,
            self.namespace.clone(),
            self.repo_name.clone(),
        )
    }
}

/// Insert payload for `replace_project_repos`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewProjectRepo {
    pub alias: String,
    pub linked_project_id: Option<i64>,
    pub namespace: Option<String>,
    pub repo_name: Option<String>,
    pub default_branch: Option<String>,
    pub mode: String,
    pub checkout: String,
    pub merge_after: Vec<String>,
    pub status: String,
    pub status_reason: Option<String>,
    pub write_granted_by: Option<i64>,
    pub write_granted_at: Option<String>,
    pub created_by: i64,
    /// Preserve the original creation time for unchanged rows; `None` = now.
    pub created_at: Option<String>,
}

impl NewProjectRepo {
    /// Repository coordinates; write permission is bound to these.
    pub fn coordinates(&self) -> (Option<i64>, Option<String>, Option<String>) {
        (
            self.linked_project_id,
            self.namespace.clone(),
            self.repo_name.clone(),
        )
    }
}

/// A `project_repos` row referencing a project, joined with the referencing project name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRepoReference {
    pub project_id: i64,
    pub project_name: String,
    pub alias: String,
}

/// A related repository fully resolved for WORKFLOW.md rendering (design 4.1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedRepo {
    pub alias: String,
    /// `namespace/repo_name` on the main project's platform instance.
    pub slug: String,
    pub default_branch: String,
    pub mode: String,
    pub checkout: String,
    pub merge_after: Vec<String>,
}

/// Audit log insert payload.
#[derive(Debug, Clone)]
pub struct NewAuditLog {
    pub actor_user_id: Option<i64>,
    pub project_id: Option<i64>,
    pub object_type: String,
    pub object_id: Option<String>,
    pub action: String,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
}

// ==================== API request/response types ====================

/// One row of `PUT /api/projects/{id}/repos`.
///
/// Source: `linked_project_id`, or a raw repository given as `git_url`
/// (or `namespace` + `repo_name`, as returned by `GET`).
#[derive(Debug, Clone, Deserialize)]
pub struct ProjectRepoInput {
    pub alias: String,
    pub linked_project_id: Option<i64>,
    pub git_url: Option<String>,
    pub namespace: Option<String>,
    pub repo_name: Option<String>,
    pub default_branch: Option<String>,
    pub mode: Option<String>,
    pub checkout: Option<String>,
    pub merge_after: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReplaceProjectReposRequest {
    pub repos: Vec<ProjectRepoInput>,
}

/// One row of `GET /api/projects/{id}/repos`.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectRepoView {
    pub id: i64,
    pub alias: String,
    /// `linked` or `raw`.
    pub source: String,
    pub linked_project_id: Option<i64>,
    pub linked_project_name: Option<String>,
    pub namespace: Option<String>,
    pub repo_name: Option<String>,
    pub default_branch: Option<String>,
    /// Value used when `default_branch` is empty (linked project's branch).
    pub inherited_default_branch: Option<String>,
    pub mode: String,
    pub checkout: String,
    pub merge_after: Vec<String>,
    pub status: String,
    pub status_reason: Option<String>,
    pub write_granted_by: Option<i64>,
    pub write_granted_at: Option<String>,
    pub created_by: i64,
    pub created_at: String,
}

/// One row of `GET /api/projects/{id}/repos/candidates`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRepoCandidate {
    pub id: i64,
    pub name: String,
}
