//! GitHub implementation of GitHost trait.

use async_trait::async_trait;
use base64::Engine;
use reqwest::{header, Client};
use serde_json::{json, Value};

use super::git_host::{GitHost, GitHostError, IssueInfo, PrInfo, Result};

pub struct GithubHost {
    token: String,
    repo: String,
    client: Client,
}

impl GithubHost {
    pub fn from_env() -> Self {
        super::load_env();
        let token = std::env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN must be set");
        let repo = std::env::var("TEST_REPO_NAME")
            .expect("TEST_REPO_NAME must be set (e.g., 'owner/repo')");

        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", token)).unwrap(),
        );
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static("symphony-e2e-test"),
        );
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static("application/vnd.github+json"),
        );

        let client = Client::builder().default_headers(headers).build().unwrap();

        Self {
            token,
            repo,
            client,
        }
    }

    fn api_url(&self, path: &str) -> String {
        format!("https://api.github.com/repos/{}{}", self.repo, path)
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
impl GitHost for GithubHost {
    async fn create_issue(&self, title: &str, body: &str, labels: &[&str]) -> Result<IssueInfo> {
        let resp = self
            .client
            .post(self.api_url("/issues"))
            .json(&json!({
                "title": title,
                "body": body,
                "labels": labels
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
            .get(self.api_url(&format!("/git/ref/heads/{}", branch)))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        let data = self.check_response(resp).await?;
        data["object"]["sha"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| GitHostError::Other("missing sha in response".into()))
    }

    async fn create_branch(&self, branch_name: &str, from_sha: &str) -> Result<()> {
        let resp = self
            .client
            .post(self.api_url("/git/refs"))
            .json(&json!({
                "ref": format!("refs/heads/{}", branch_name),
                "sha": from_sha
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
            .delete(self.api_url(&format!("/git/refs/heads/{}", branch_name)))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        if resp.status().is_success() || resp.status().as_u16() == 422 {
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
            .put(self.api_url(&format!("/contents/{}", path)))
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
        format!(
            "https://x-access-token:{}@github.com/{}.git",
            self.token, self.repo
        )
    }

    fn platform_name(&self) -> &'static str {
        "GitHub"
    }
}
