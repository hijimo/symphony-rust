//! GitLab implementation of GitHost trait.

use async_trait::async_trait;
use base64::Engine;
use reqwest::Client;
use serde_json::{json, Value};

use super::git_host::{GitHost, GitHostError, IssueInfo, PrInfo, Result};

pub struct GitlabHost {
    token: String,
    project_path: String,
    base_url: String,
    client: Client,
}

impl GitlabHost {
    pub fn from_env() -> Self {
        super::load_env();
        let token = std::env::var("GITLAB_TOKEN").expect("GITLAB_TOKEN must be set");
        let project_path = std::env::var("TEST_REPO_NAME")
            .expect("TEST_REPO_NAME must be set (e.g., 'owner/repo')");
        let base_url =
            std::env::var("GITLAB_BASE_URL").unwrap_or_else(|_| "https://gitlab.com".to_string());
        Self {
            token,
            project_path,
            base_url,
            client: Client::builder().no_proxy().build().unwrap(),
        }
    }

    fn encoded_project(&self) -> String {
        urlencoding::encode(&self.project_path).to_string()
    }

    fn api_url(&self, path: &str) -> String {
        format!(
            "{}/api/v4/projects/{}{}",
            self.base_url,
            self.encoded_project(),
            path
        )
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
impl GitHost for GitlabHost {
    async fn create_issue(&self, title: &str, body: &str, labels: &[&str]) -> Result<IssueInfo> {
        let resp = self
            .client
            .post(self.api_url("/issues"))
            .header("PRIVATE-TOKEN", &self.token)
            .json(&json!({
                "title": title,
                "description": body,
                "labels": labels.join(",")
            }))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        let data = self.check_response(resp).await?;
        Ok(IssueInfo {
            id: data["id"].as_u64().unwrap_or(0),
            number: data["iid"].as_u64().unwrap_or(0),
            url: data["web_url"].as_str().unwrap_or("").to_string(),
        })
    }

    async fn close_issue(&self, issue_number: u64) -> Result<()> {
        let resp = self
            .client
            .put(self.api_url(&format!("/issues/{}", issue_number)))
            .header("PRIVATE-TOKEN", &self.token)
            .json(&json!({ "state_event": "close" }))
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
        let encoded_branch = urlencoding::encode(branch);
        let resp = self
            .client
            .get(self.api_url(&format!("/repository/branches/{}", encoded_branch)))
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        let data = self.check_response(resp).await?;
        data["commit"]["id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| GitHostError::Other("missing commit.id in response".into()))
    }

    async fn create_branch(&self, branch_name: &str, from_sha: &str) -> Result<()> {
        let resp = self
            .client
            .post(self.api_url("/repository/branches"))
            .header("PRIVATE-TOKEN", &self.token)
            .json(&json!({
                "branch": branch_name,
                "ref": from_sha
            }))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        self.check_response(resp).await?;
        Ok(())
    }

    async fn delete_branch(&self, branch_name: &str) -> Result<()> {
        let encoded_branch = urlencoding::encode(branch_name);
        let resp = self
            .client
            .delete(self.api_url(&format!("/repository/branches/{}", encoded_branch)))
            .header("PRIVATE-TOKEN", &self.token)
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
        let encoded_path = urlencoding::encode(path);
        let encoded_content = base64::engine::general_purpose::STANDARD.encode(content);

        let resp = self
            .client
            .post(self.api_url(&format!("/repository/files/{}", encoded_path)))
            .header("PRIVATE-TOKEN", &self.token)
            .json(&json!({
                "branch": branch,
                "content": encoded_content,
                "commit_message": commit_msg,
                "encoding": "base64"
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
            .post(self.api_url("/merge_requests"))
            .header("PRIVATE-TOKEN", &self.token)
            .json(&json!({
                "title": title,
                "description": body,
                "source_branch": head,
                "target_branch": base
            }))
            .send()
            .await
            .map_err(|e| GitHostError::Http(e.to_string()))?;

        let data = self.check_response(resp).await?;
        Ok(PrInfo {
            number: data["iid"].as_u64().unwrap_or(0),
            url: data["web_url"].as_str().unwrap_or("").to_string(),
        })
    }

    fn clone_url(&self) -> String {
        let url_with_auth = if self.base_url.starts_with("https://") {
            self.base_url
                .replace("https://", &format!("https://oauth2:{}@", self.token))
        } else {
            self.base_url
                .replace("http://", &format!("http://oauth2:{}@", self.token))
        };
        format!("{}/{}.git", url_with_auth, self.project_path)
    }

    fn platform_name(&self) -> &'static str {
        "GitLab"
    }
}

// ─── Extended GitLab operations (not part of GitHost trait) ─────────────────

impl GitlabHost {
    pub async fn get_issue_labels(&self, iid: u64) -> super::git_host::Result<Vec<String>> {
        let resp = self
            .client
            .get(self.api_url(&format!("/issues/{}", iid)))
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await
            .map_err(|e| super::git_host::GitHostError::Http(e.to_string()))?;

        let data = self.check_response(resp).await?;
        let labels = data["labels"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(labels)
    }

    pub async fn add_label(&self, iid: u64, label: &str) -> super::git_host::Result<()> {
        let current_labels = self.get_issue_labels(iid).await?;
        let mut all_labels = current_labels;
        if !all_labels.iter().any(|l| l == label) {
            all_labels.push(label.to_string());
        }

        let resp = self
            .client
            .put(self.api_url(&format!("/issues/{}", iid)))
            .header("PRIVATE-TOKEN", &self.token)
            .json(&serde_json::json!({
                "labels": all_labels.join(",")
            }))
            .send()
            .await
            .map_err(|e| super::git_host::GitHostError::Http(e.to_string()))?;

        self.check_response(resp).await?;
        Ok(())
    }

    pub async fn get_issue_notes(
        &self,
        iid: u64,
    ) -> super::git_host::Result<Vec<serde_json::Value>> {
        let resp = self
            .client
            .get(self.api_url(&format!("/issues/{}/notes?per_page=100", iid)))
            .header("PRIVATE-TOKEN", &self.token)
            .send()
            .await
            .map_err(|e| super::git_host::GitHostError::Http(e.to_string()))?;

        let data = self.check_response(resp).await?;
        let notes = data
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter(|n| !n["system"].as_bool().unwrap_or(false))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        Ok(notes)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn project_path(&self) -> &str {
        &self.project_path
    }
}
