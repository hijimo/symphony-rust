use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    GitHub,
    GitLab,
    Gitea,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::GitHub => write!(f, "github"),
            Platform::GitLab => write!(f, "gitlab"),
            Platform::Gitea => write!(f, "gitea"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedGitUrl {
    pub platform: Platform,
    pub host: String,
    pub namespace: String,
    pub repo_name: String,
    pub normalized_url: String,
}

#[derive(Debug, Error)]
pub enum GitUrlError {
    #[error("Invalid Git URL format: {0}")]
    InvalidFormat(String),
    #[error("Missing namespace or repository name in URL")]
    MissingComponents,
    #[error("Empty URL")]
    Empty,
}

/// Parse a Git URL into its components.
///
/// Supports:
/// - HTTPS: `https://github.com/owner/repo`, `https://gitlab.com/group/sub/project`
/// - SSH: `git@github.com:owner/repo.git`, `git@gitlab.com:group/project.git`
/// - Custom domains (default to GitLab)
/// - Trailing slashes, `.git` suffix
pub fn parse_git_url(url: &str) -> Result<ParsedGitUrl, GitUrlError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(GitUrlError::Empty);
    }

    // Try SSH format first: git@host:path.git
    let ssh_re = Regex::new(r"^git@([^:]+):(.+)$").unwrap();
    if let Some(caps) = ssh_re.captures(url) {
        let host = caps.get(1).unwrap().as_str().to_string();
        let path = caps.get(2).unwrap().as_str();
        return parse_path_components(&host, path);
    }

    // Try HTTPS format: https://host/path
    let https_re = Regex::new(r"^https?://([^/]+)/(.+)$").unwrap();
    if let Some(caps) = https_re.captures(url) {
        let authority = caps.get(1).unwrap().as_str();
        // Strip `user:pass@` / `user@` userinfo (e.g. `oauth2:<token>@`).
        let host = strip_userinfo(authority).to_string();
        if host.is_empty() {
            return Err(GitUrlError::InvalidFormat(url.to_string()));
        }
        let path = caps.get(2).unwrap().as_str();
        return parse_path_components(&host, path);
    }

    Err(GitUrlError::InvalidFormat(url.to_string()))
}

/// Remove the userinfo part (`user[:password]@`) from a URL authority.
fn strip_userinfo(authority: &str) -> &str {
    match authority.rfind('@') {
        Some(idx) => &authority[idx + 1..],
        None => authority,
    }
}

/// Canonical, lowercase `host/namespace/repo_name` key of a Git URL.
pub fn repo_identity_key(url: &str) -> Option<String> {
    parse_git_url(url).ok().map(|p| {
        format!(
            "{}/{}/{}",
            p.host.to_lowercase(),
            p.namespace.to_lowercase(),
            p.repo_name.to_lowercase()
        )
    })
}

/// Whether two Git URLs point to the same repository.
///
/// Compares lowercase `host + namespace + repo_name`; scheme, userinfo,
/// `.git` suffix and SSH/HTTP form are ignored. Unparseable URLs never match.
pub fn same_repo(a: &str, b: &str) -> bool {
    match (repo_identity_key(a), repo_identity_key(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

fn parse_path_components(host: &str, path: &str) -> Result<ParsedGitUrl, GitUrlError> {
    // Clean up path: remove trailing slashes and .git suffix
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let path = path.trim_end_matches('/');

    if path.is_empty() {
        return Err(GitUrlError::MissingComponents);
    }

    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 2 {
        return Err(GitUrlError::MissingComponents);
    }

    let repo_name = parts.last().unwrap().to_string();
    let namespace = parts[..parts.len() - 1].join("/");

    let platform = detect_platform(host);
    let normalized_url = format!("https://{}/{}/{}", host, namespace, repo_name);

    Ok(ParsedGitUrl {
        platform,
        host: host.to_string(),
        namespace,
        repo_name,
        normalized_url,
    })
}

fn detect_platform(host: &str) -> Platform {
    let host_lower = host.to_lowercase();
    if host_lower.contains("github.com") {
        Platform::GitHub
    } else if host_lower.contains("gitlab") {
        Platform::GitLab
    } else {
        // Default to GitLab for custom/self-hosted domains
        Platform::GitLab
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_https_github() {
        let result = parse_git_url("https://github.com/owner/repo").unwrap();
        assert_eq!(result.platform, Platform::GitHub);
        assert_eq!(result.host, "github.com");
        assert_eq!(result.namespace, "owner");
        assert_eq!(result.repo_name, "repo");
        assert_eq!(result.normalized_url, "https://github.com/owner/repo");
    }

    #[test]
    fn test_https_github_with_git_suffix() {
        let result = parse_git_url("https://github.com/owner/repo.git").unwrap();
        assert_eq!(result.platform, Platform::GitHub);
        assert_eq!(result.namespace, "owner");
        assert_eq!(result.repo_name, "repo");
    }

    #[test]
    fn test_https_github_trailing_slash() {
        let result = parse_git_url("https://github.com/owner/repo/").unwrap();
        assert_eq!(result.platform, Platform::GitHub);
        assert_eq!(result.namespace, "owner");
        assert_eq!(result.repo_name, "repo");
    }

    #[test]
    fn test_https_gitlab() {
        let result = parse_git_url("https://gitlab.com/group/project").unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.host, "gitlab.com");
        assert_eq!(result.namespace, "group");
        assert_eq!(result.repo_name, "project");
    }

    #[test]
    fn test_https_gitlab_nested_groups() {
        let result = parse_git_url("https://gitlab.com/group/sub/project").unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.namespace, "group/sub");
        assert_eq!(result.repo_name, "project");
        assert_eq!(
            result.normalized_url,
            "https://gitlab.com/group/sub/project"
        );
    }

    #[test]
    fn test_https_custom_gitlab() {
        let result = parse_git_url("https://gitlab.example.com/group/project").unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.host, "gitlab.example.com");
        assert_eq!(result.namespace, "group");
        assert_eq!(result.repo_name, "project");
    }

    #[test]
    fn test_https_custom_domain_defaults_to_gitlab() {
        let result = parse_git_url("https://git.mycompany.com/team/service").unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.host, "git.mycompany.com");
        assert_eq!(result.namespace, "team");
        assert_eq!(result.repo_name, "service");
    }

    #[test]
    fn test_ssh_github() {
        let result = parse_git_url("git@github.com:owner/repo.git").unwrap();
        assert_eq!(result.platform, Platform::GitHub);
        assert_eq!(result.host, "github.com");
        assert_eq!(result.namespace, "owner");
        assert_eq!(result.repo_name, "repo");
        assert_eq!(result.normalized_url, "https://github.com/owner/repo");
    }

    #[test]
    fn test_ssh_gitlab() {
        let result = parse_git_url("git@gitlab.com:group/project.git").unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.host, "gitlab.com");
        assert_eq!(result.namespace, "group");
        assert_eq!(result.repo_name, "project");
    }

    #[test]
    fn test_ssh_without_git_suffix() {
        let result = parse_git_url("git@github.com:owner/repo").unwrap();
        assert_eq!(result.platform, Platform::GitHub);
        assert_eq!(result.namespace, "owner");
        assert_eq!(result.repo_name, "repo");
    }

    #[test]
    fn test_ssh_nested_gitlab() {
        let result = parse_git_url("git@gitlab.com:group/sub/project.git").unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.namespace, "group/sub");
        assert_eq!(result.repo_name, "project");
    }

    #[test]
    fn test_empty_url() {
        let result = parse_git_url("");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), GitUrlError::Empty));
    }

    #[test]
    fn test_whitespace_url() {
        let result = parse_git_url("   ");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), GitUrlError::Empty));
    }

    #[test]
    fn test_invalid_format() {
        let result = parse_git_url("not-a-url");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), GitUrlError::InvalidFormat(_)));
    }

    #[test]
    fn test_missing_repo_name() {
        let result = parse_git_url("https://github.com/owner");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GitUrlError::MissingComponents
        ));
    }

    #[test]
    fn test_http_url() {
        let result = parse_git_url("http://gitlab.com/group/project").unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.namespace, "group");
        assert_eq!(result.repo_name, "project");
        // Normalized URL always uses https
        assert_eq!(result.normalized_url, "https://gitlab.com/group/project");
    }

    #[test]
    fn test_http_url_with_token_userinfo() {
        let result = parse_git_url(
            "http://oauth2:glpat-xxx@gitlab.jushuitan-inc.com:8081/zimei10525/jst-ai-studio.git",
        )
        .unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.host, "gitlab.jushuitan-inc.com:8081");
        assert_eq!(result.namespace, "zimei10525");
        assert_eq!(result.repo_name, "jst-ai-studio");
        assert_eq!(
            result.normalized_url,
            "https://gitlab.jushuitan-inc.com:8081/zimei10525/jst-ai-studio"
        );
    }

    #[test]
    fn test_https_url_with_user_only_userinfo() {
        let result = parse_git_url("https://deploy@github.com/owner/repo.git").unwrap();
        assert_eq!(result.platform, Platform::GitHub);
        assert_eq!(result.host, "github.com");
        assert_eq!(result.namespace, "owner");
        assert_eq!(result.repo_name, "repo");
    }

    #[test]
    fn test_userinfo_without_host_is_invalid() {
        assert!(matches!(
            parse_git_url("https://user@/owner/repo"),
            Err(GitUrlError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_ssh_still_works_with_userinfo_logic() {
        let result = parse_git_url("git@gitlab.example.com:group/sub/project.git").unwrap();
        assert_eq!(result.host, "gitlab.example.com");
        assert_eq!(result.namespace, "group/sub");
        assert_eq!(result.repo_name, "project");
    }

    #[test]
    fn test_same_repo() {
        assert!(same_repo(
            "http://oauth2:glpat-xxx@gitlab.jushuitan-inc.com:8081/zimei10525/jst-ai-studio.git",
            "https://gitlab.jushuitan-inc.com:8081/zimei10525/jst-ai-studio"
        ));
        assert!(same_repo(
            "git@GitHub.com:Owner/Repo.git",
            "https://github.com/owner/repo/"
        ));
        assert!(!same_repo(
            "https://gitlab.com/group/a",
            "https://gitlab.com/group/b"
        ));
        assert!(!same_repo(
            "https://gitlab.com:8081/group/a",
            "https://gitlab.com/group/a"
        ));
        assert!(!same_repo("not-a-url", "not-a-url"));
    }
}
