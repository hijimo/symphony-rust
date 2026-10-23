//! Minimal GitLab REST calls used by the multi-repo runtime (design §5.1, §5.3).
//!
//! Kept separate from the `Platform` trait on purpose: these are only needed
//! in multi-repo mode, so the five existing adapters stay untouched.

use async_trait::async_trait;
use serde::Deserialize;

/// Remote operations the multi-repo runtime needs. Faked in tests.
#[async_trait]
pub trait RepoApi: Send + Sync {
    /// Highest id of an `add` event of `label` on the primary issue.
    async fn latest_label_add_event(
        &self,
        issue_iid: &str,
        label: &str,
    ) -> Result<Option<u64>, String>;
    /// Open MRs whose source branch is exactly `branch` in project `slug`.
    async fn open_mrs_for_branch(&self, slug: &str, branch: &str) -> Result<Vec<u64>, String>;
    /// Close MR `iid` in project `slug` (no-op if already closed).
    async fn close_mr(&self, slug: &str, iid: u64) -> Result<(), String>;
}

pub struct GitlabRepoApi {
    client: reqwest::Client,
    /// `<host>/api/v4`
    api_base: String,
    token: String,
    /// Primary project slug (issues live there).
    primary_slug: String,
}

impl GitlabRepoApi {
    pub fn new(api_base: &str, token: &str, primary_slug: &str) -> Result<Self, String> {
        let client = crate::proxy::proxy_aware_client_builder()
            .map_err(|e| e.to_string())?
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            api_base: api_base.trim_end_matches('/').to_string(),
            token: token.to_string(),
            primary_slug: primary_slug.to_string(),
        })
    }

    async fn get_pages<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Vec<T>, String> {
        let mut out = Vec::new();
        for page in 1..=50u32 {
            let page_s = page.to_string();
            let mut q: Vec<(&str, &str)> = query.to_vec();
            q.push(("per_page", "100"));
            q.push(("page", &page_s));
            let resp = self
                .client
                .get(format!("{}{}", self.api_base, path))
                .header("PRIVATE-TOKEN", &self.token)
                .query(&q)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("GET {path} -> {}", resp.status()));
            }
            let next = resp
                .headers()
                .get("x-next-page")
                .and_then(|v| v.to_str().ok())
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false);
            let items: Vec<T> = resp.json().await.map_err(|e| e.to_string())?;
            let len = items.len();
            out.extend(items);
            if !next || len < 100 {
                break;
            }
        }
        Ok(out)
    }
}

fn enc(slug: &str) -> String {
    urlencoding::encode(slug).into_owned()
}

#[derive(Deserialize)]
struct LabelEvent {
    id: u64,
    action: String,
    label: Option<LabelRef>,
}

#[derive(Deserialize)]
struct LabelRef {
    name: String,
}

#[derive(Deserialize)]
struct MrRef {
    iid: u64,
}

/// Pure selection logic for label events (unit-tested).
pub fn latest_add(
    events: impl IntoIterator<Item = (u64, String, Option<String>)>,
    label: &str,
) -> Option<u64> {
    events
        .into_iter()
        .filter(|(_, action, name)| {
            action == "add"
                && name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(label))
        })
        .map(|(id, _, _)| id)
        .max()
}

#[async_trait]
impl RepoApi for GitlabRepoApi {
    async fn latest_label_add_event(
        &self,
        issue_iid: &str,
        label: &str,
    ) -> Result<Option<u64>, String> {
        let path = format!(
            "/projects/{}/issues/{}/resource_label_events",
            enc(&self.primary_slug),
            issue_iid
        );
        let events: Vec<LabelEvent> = self.get_pages(&path, &[]).await?;
        Ok(latest_add(
            events
                .into_iter()
                .map(|e| (e.id, e.action, e.label.map(|l| l.name))),
            label,
        ))
    }

    async fn open_mrs_for_branch(&self, slug: &str, branch: &str) -> Result<Vec<u64>, String> {
        let path = format!("/projects/{}/merge_requests", enc(slug));
        let mrs: Vec<MrRef> = self
            .get_pages(&path, &[("state", "opened"), ("source_branch", branch)])
            .await?;
        Ok(mrs.into_iter().map(|m| m.iid).collect())
    }

    async fn close_mr(&self, slug: &str, iid: u64) -> Result<(), String> {
        let resp = self
            .client
            .put(format!(
                "{}/projects/{}/merge_requests/{}",
                self.api_base,
                enc(slug),
                iid
            ))
            .header("PRIVATE-TOKEN", &self.token)
            .json(&serde_json::json!({ "state_event": "close" }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("close MR !{iid} in {slug} -> {}", resp.status()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_add_picks_max_add_of_label_only() {
        let ev = vec![
            (6957, "add".to_string(), Some("Todo".to_string())),
            (7001, "add".to_string(), Some("Rework".to_string())),
            (7002, "remove".to_string(), Some("Rework".to_string())),
            (7100, "add".to_string(), Some("rework".to_string())),
            (7200, "add".to_string(), None),
        ];
        assert_eq!(latest_add(ev.clone(), "Rework"), Some(7100));
        assert_eq!(latest_add(ev, "Merging"), None);
    }
}
