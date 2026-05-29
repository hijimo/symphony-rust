//! Gitea implementation of GitHost trait.

use async_trait::async_trait;
use base64::Engine;
use reqwest::{header, Client};
use serde_json::{json, Value};

use super::git_host::{GitHost, GitHostError, IssueInfo, PrInfo, Result};

pub struct GiteaHost {
    token: String,
    repo: String,
    base_url: String,
    client: Client,
}

impl GiteaHost {
    pub fn from_env() -> Self {
        super::load_env();
        let token = std::env::var("GITEA_TOKEN").expect("GITEA_TOKEN must be set");
        let repo = std::env::var("TEST_REPO_NAME")
            .expect("TEST_REPO_NAME must be set (e.g., 'owner/repo')");
        let base_url = std::env::var("GITEA_BASE_URL")
            .expect("GITEA_BASE_URL must be set (e.g., 'https://gitea.example.com/api/v1')");

        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("token {}", token)).unwrap(),
        );
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static("symphony-e2e-test"),
        );
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static("application/json"),
        );

        let client = Client::builder().default_headers(headers).build().unwrap();

        Self {
            token,
            repo,
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        }
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/repos/{}{}", self.base_url, self.repo, path)
    }

    async fn check_response(&self, resp: reqwest::Response) -> Result<Value> {
        let status = resp.status();
        if status.is_success() {
            let body: Value = resp
                .json()
                .await
                .map_err(|e| GitHostError::Http(e.to_string()))?;
            Ok(body)
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(GitHostError::Api {
                status: status.as_u16(),
                body,
            })
        }
    }
}

#[async_trait]
impl GitHost for GiteaHost {
    async fn create_issue(&self, title: &str, body: &str, labels: &[&str]) -> Result<IssueInfo> {
        // Resolve label names to IDs
        let labels_resp = self
            .client
            .get(self.api_url("/labels"))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;
        let all_labels: Vec<Value> = labels_resp
            .json()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        let label_ids: Vec<u64> = labels
            .iter()
            .filter_map(|name| {
                all_labels
                    .iter()
                    .find(|l| l["name"].as_str() == Some(name))
                    .and_then(|l| l["id"].as_u64())
            })
            .collect();

        let resp = self
            .client
            .post(self.api_url("/issues"))
            .json(&json!({
                "title": title,
                "body": body,
                "labels": label_ids
            }))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        let data = self.check_response(resp).await?;
        Ok(IssueInfo {
            id: data["id"].as_u64().unwrap_or(0),
            number: data["number"].as_u64().unwrap_or(0),
            url: data["html_url"].as_str().unwrap_or("").to_string(),
        })
    }

    async fn close_issue(&self, issue_number: u64) -> Result<()> {
        let resp = self
            .client
            .patch(self.api_url(&format!("/issues/{}", issue_number)))
            .json(&json!({ "state": "closed" }))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(GitHostError::Api { status, body })
        }
    }

    async fn get_branch_sha(&self, branch: &str) -> Result<String> {
        let resp = self
            .client
            .get(self.api_url(&format!("/branches/{}", branch)))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        let data = self.check_response(resp).await?;
        data["commit"]["id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| GitHostError::Other("missing commit.id in branch response".into()))
    }

    async fn create_branch(&self, branch_name: &str, from_sha: &str) -> Result<()> {
        // Gitea's branch API uses old_branch_name (a branch name, not a SHA).
        // If from_sha looks like a branch name, use it directly; otherwise use "main".
        let old_branch = if from_sha.len() == 40 && from_sha.chars().all(|c| c.is_ascii_hexdigit())
        {
            "main"
        } else {
            from_sha
        };
        let resp = self
            .client
            .post(self.api_url("/branches"))
            .json(&json!({
                "new_branch_name": branch_name,
                "old_branch_name": old_branch
            }))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        self.check_response(resp).await?;
        Ok(())
    }

    async fn delete_branch(&self, branch_name: &str) -> Result<()> {
        let resp = self
            .client
            .delete(self.api_url(&format!("/branches/{}", branch_name)))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        if resp.status().is_success() || resp.status().as_u16() == 404 {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(GitHostError::Api { status, body })
        }
    }

    async fn push_file(
        &self,
        branch: &str,
        path: &str,
        content: &[u8],
        commit_msg: &str,
    ) -> Result<()> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(content);
        let resp = self
            .client
            .post(self.api_url(&format!("/contents/{}", path)))
            .json(&json!({
                "message": commit_msg,
                "content": encoded,
                "branch": branch
            }))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        self.check_response(resp).await?;
        Ok(())
    }

    async fn create_pr(&self, title: &str, body: &str, head: &str, base: &str) -> Result<PrInfo> {
        let resp = self
            .client
            .post(self.api_url("/pulls"))
            .json(&json!({
                "title": title,
                "body": body,
                "head": head,
                "base": base
            }))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        let data = self.check_response(resp).await?;
        Ok(PrInfo {
            number: data["number"].as_u64().unwrap_or(0),
            url: data["html_url"].as_str().unwrap_or("").to_string(),
        })
    }

    fn clone_url(&self) -> String {
        // base_url is like "https://omv.iloxe.com:23000/api/v1"
        // We need "https://token@omv.iloxe.com:23000/owner/repo.git"
        let url_without_api = self
            .base_url
            .trim_end_matches("/api/v1")
            .trim_end_matches("/api/v1/");
        // Insert token into URL: https://host -> https://token@host
        let clone = if let Some(rest) = url_without_api.strip_prefix("https://") {
            format!("https://{}@{}/{}.git", self.token, rest, self.repo)
        } else if let Some(rest) = url_without_api.strip_prefix("http://") {
            format!("http://{}@{}/{}.git", self.token, rest, self.repo)
        } else {
            format!("{}/{}.git", url_without_api, self.repo)
        };
        clone
    }

    fn platform_name(&self) -> &'static str {
        "Gitea"
    }
}

// Extra helper methods for E2E tests (not part of GitHost trait)
impl GiteaHost {
    pub async fn get_issue_labels(&self, issue_number: u64) -> Result<Vec<String>> {
        let resp = self
            .client
            .get(self.api_url(&format!("/issues/{}", issue_number)))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        let data = self.check_response(resp).await?;
        let labels = data["labels"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|l| l["name"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(labels)
    }

    pub async fn get_issue_comments(&self, issue_number: u64) -> Result<Vec<Value>> {
        let resp = self
            .client
            .get(self.api_url(&format!("/issues/{}/comments", issue_number)))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        let data = self.check_response(resp).await?;
        Ok(data.as_array().cloned().unwrap_or_default())
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn project_path(&self) -> &str {
        &self.repo
    }
}
