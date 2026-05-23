//! GitHost trait — abstracts git hosting platform operations for E2E tests.
//!
//! Implementations: GithubHost, GitlabHost

use async_trait::async_trait;

#[derive(Debug)]
pub struct IssueInfo {
    pub id: u64,
    pub number: u64,
    pub url: String,
}

#[derive(Debug)]
pub struct PrInfo {
    pub number: u64,
    pub url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum GitHostError {
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("API error ({status}): {body}")]
    Api { status: u16, body: String },
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, GitHostError>;

#[async_trait]
pub trait GitHost: Send + Sync {
    /// Create an issue with title, body, and labels.
    async fn create_issue(&self, title: &str, body: &str, labels: &[&str]) -> Result<IssueInfo>;

    /// Close an issue by its platform-native number/iid.
    async fn close_issue(&self, issue_number: u64) -> Result<()>;

    /// Get the SHA of a branch HEAD.
    async fn get_branch_sha(&self, branch: &str) -> Result<String>;

    /// Create a new branch pointing at the given SHA.
    async fn create_branch(&self, branch_name: &str, from_sha: &str) -> Result<()>;

    /// Delete a branch by name.
    async fn delete_branch(&self, branch_name: &str) -> Result<()>;

    /// Push (create/update) a file on a branch via the platform API.
    async fn push_file(
        &self,
        branch: &str,
        path: &str,
        content: &[u8],
        commit_msg: &str,
    ) -> Result<()>;

    /// Create a pull request / merge request.
    async fn create_pr(&self, title: &str, body: &str, head: &str, base: &str) -> Result<PrInfo>;

    /// Returns a clone URL with embedded authentication.
    fn clone_url(&self) -> String;

    /// Platform display name for logging.
    fn platform_name(&self) -> &'static str;
}

/// Factory: create the appropriate GitHost based on E2E_PLATFORM env var.
pub fn create_git_host() -> Box<dyn GitHost> {
    let platform = std::env::var("E2E_PLATFORM").unwrap_or_else(|_| "github".to_string());
    match platform.as_str() {
        "gitlab" => Box::new(super::gitlab_host::GitlabHost::from_env()),
        _ => Box::new(super::github_host::GithubHost::from_env()),
    }
}
