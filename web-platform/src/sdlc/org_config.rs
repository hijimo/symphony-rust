//! Organization rules repository (docs/方案/ai-native-sdlc-p3-design.md §4 O1):
//! a bare mirror is fetched periodically and each needed commit is exported
//! file by file to `<workspace_root>/org-config/<sha>/`. Nothing from the
//! repository is executed (no checkout, hooks, submodules, LFS or filters).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use rusqlite::params;
use serde::Serialize;
use tokio::sync::Mutex;

use super::workspace::WorkspaceService;
use crate::db::DbPool;
use crate::error::WebPlatformError;
use crate::repository::{ProjectRepoRepository, SqliteRepository, SystemConfigRepository};

pub const KEY_URL: &str = "org_config.url";
pub const KEY_REF: &str = "org_config.ref";
pub const KEY_INTERVAL: &str = "org_config.sync_interval_secs";
/// Internal: last successfully exported commit of `org_config.ref`.
pub const KEY_CURRENT: &str = "org_config.current_sha";
/// Read-only clone credential (never stored in the database).
pub const TOKEN_ENV: &str = "SYMPHONY_ORG_CONFIG_TOKEN";

pub const MAX_FILE_BYTES: i64 = 1024 * 1024;
pub const MAX_TOTAL_BYTES: i64 = 20 * 1024 * 1024;
/// Exported versions kept besides the current and pinned ones.
pub const KEEP_VERSIONS: usize = 5;
const MIRROR_NAME: &str = "org-config";

/// File names that must never be in the repository (credentials).
fn forbidden(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "auth.json"
        || lower == ".env"
        || lower.starts_with(".env.")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".p12")
        || lower == "id_rsa"
        || lower == "id_ed25519"
}

/// Clone URL accepted from the settings (no option injection).
pub fn valid_url(url: &str) -> bool {
    let u = url.trim();
    !u.is_empty()
        && !u.starts_with('-')
        && !u.chars().any(|c| c.is_whitespace() || c.is_control())
        && (u.starts_with("https://")
            || u.starts_with("http://")
            || u.starts_with("ssh://")
            || u.starts_with("git@")
            || u.starts_with('/'))
}

/// Branch, tag or commit name accepted as ref / pin.
pub fn valid_ref(r: &str) -> bool {
    !r.is_empty()
        && r.len() <= 200
        && !r.starts_with('-')
        && !r.contains("..")
        && r.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PinStatus {
    pub project_id: i64,
    pub pin: String,
    pub sha: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub enabled: bool,
    pub url: Option<String>,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub interval_secs: u64,
    pub current_sha: Option<String>,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    pub pins: Vec<PinStatus>,
    /// Exported commits on disk.
    pub versions: Vec<String>,
}

struct Settings {
    url: Option<String>,
    git_ref: String,
    interval_secs: u64,
    current: Option<String>,
}

#[derive(Clone)]
pub struct OrgConfigService {
    root: PathBuf,
    workspaces: WorkspaceService,
    pool: DbPool,
    last: Arc<Mutex<(Option<String>, Option<String>)>>,
    sync_lock: Arc<Mutex<()>>,
}

impl OrgConfigService {
    pub fn new(workspace_root: &str, workspaces: WorkspaceService, pool: DbPool) -> Self {
        Self {
            root: Path::new(workspace_root).join("org-config"),
            workspaces,
            pool,
            last: Arc::new(Mutex::new((None, None))),
            sync_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Directory of an exported commit.
    pub fn version_dir(&self, sha: &str) -> PathBuf {
        self.root.join(sha)
    }

    async fn settings(&self, repo: &SqliteRepository) -> Result<Settings, WebPlatformError> {
        let all: BTreeMap<String, String> = repo
            .list_system_configs()
            .await?
            .into_iter()
            .map(|c| (c.key, c.value))
            .collect();
        let url = all
            .get(KEY_URL)
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty() && u != "-");
        Ok(Settings {
            url,
            git_ref: all.get(KEY_REF).cloned().unwrap_or_else(|| "main".into()),
            interval_secs: all
                .get(KEY_INTERVAL)
                .and_then(|v| v.parse().ok())
                .unwrap_or(300)
                .max(30),
            current: all.get(KEY_CURRENT).cloned().filter(|s| !s.is_empty()),
        })
    }

    fn pins(&self) -> Result<Vec<(i64, String)>, WebPlatformError> {
        let c = self.pool.get()?;
        let mut stmt = c.prepare(
            "SELECT id, org_config_pin FROM projects WHERE org_config_pin IS NOT NULL AND org_config_pin != '' ORDER BY id",
        )?;
        let rows = stmt.query_map(params![], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn versions(&self) -> Vec<String> {
        let mut v: Vec<(std::time::SystemTime, String)> = std::fs::read_dir(&self.root)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| n.len() == 40 && n.chars().all(|c| c.is_ascii_hexdigit()))
                    .map(|n| {
                        let t = std::fs::metadata(self.root.join(&n))
                            .and_then(|m| m.modified())
                            .unwrap_or(std::time::UNIX_EPOCH);
                        (t, n)
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v.into_iter().map(|(_, n)| n).collect()
    }

    /// Current status without syncing.
    pub async fn status(&self, repo: &SqliteRepository) -> Result<SyncStatus, WebPlatformError> {
        let st = self.settings(repo).await?;
        let last = self.last.lock().await.clone();
        Ok(SyncStatus {
            enabled: st.url.is_some(),
            url: st.url,
            git_ref: st.git_ref,
            interval_secs: st.interval_secs,
            current_sha: st.current,
            last_sync_at: last.0,
            last_error: last.1,
            pins: self
                .pins()?
                .into_iter()
                .map(|(project_id, pin)| PinStatus {
                    project_id,
                    pin,
                    sha: None,
                    error: None,
                })
                .collect(),
            versions: self.versions(),
        })
    }

    /// Fetch, export the followed ref and every pinned ref, prune old
    /// exports. A failure keeps the previous version current.
    pub async fn sync(&self, repo: &SqliteRepository) -> Result<SyncStatus, WebPlatformError> {
        let _g = self.sync_lock.lock().await;
        let st = self.settings(repo).await?;
        let Some(url) = st.url.clone() else {
            return self.status(repo).await;
        };
        let now = chrono::Utc::now().to_rfc3339();
        let mut pins = Vec::new();
        let result: Result<String, String> = async {
            if !valid_url(&url) {
                return Err(
                    "仓库地址无效：只支持 https://、http://、ssh://、git@ 或本机绝对路径"
                        .to_string(),
                );
            }
            if !valid_ref(&st.git_ref) {
                return Err(format!("ref 无效：{}", st.git_ref));
            }
            let token = std::env::var(TOKEN_ENV).ok().filter(|t| !t.is_empty());
            let mirror = self
                .workspaces
                .sync_shared_mirror(MIRROR_NAME, &url, token.as_deref())
                .await
                .map_err(|e| format!("同步失败：{e}"))?;
            let sha = self
                .resolve(&mirror, &st.git_ref)
                .await
                .ok_or_else(|| format!("找不到 ref {}", st.git_ref))?;
            self.export(&mirror, &sha).await?;
            for (project_id, pin) in self.pins().map_err(|e| e.to_string())? {
                let (sha, error) = if !valid_ref(&pin) {
                    (None, Some(format!("锁定的版本无效：{pin}")))
                } else {
                    match self.resolve(&mirror, &pin).await {
                        None => (None, Some(format!("找不到锁定的版本 {pin}"))),
                        Some(s) => match self.export(&mirror, &s).await {
                            Ok(()) => (Some(s), None),
                            Err(e) => (None, Some(e)),
                        },
                    }
                };
                pins.push(PinStatus {
                    project_id,
                    pin,
                    sha,
                    error,
                });
            }
            Ok(sha)
        }
        .await;
        let error = match result {
            Ok(sha) => {
                repo.upsert_system_config(KEY_CURRENT, &sha).await?;
                let keep: BTreeSet<String> = std::iter::once(sha)
                    .chain(pins.iter().filter_map(|p| p.sha.clone()))
                    .collect();
                self.prune(&keep);
                None
            }
            Err(e) => {
                tracing::warn!("organization rules sync failed: {e}");
                Some(e)
            }
        };
        *self.last.lock().await = (Some(now), error);
        let mut status = self.status(repo).await?;
        if !pins.is_empty() {
            status.pins = pins;
        }
        Ok(status)
    }

    /// Commit sha of a remote branch, tag or commit.
    async fn resolve(&self, mirror: &Path, r: &str) -> Option<String> {
        for candidate in [
            format!("refs/remotes/origin/{r}^{{commit}}"),
            format!("refs/tags/{r}^{{commit}}"),
            format!("{r}^{{commit}}"),
        ] {
            if let Ok(out) = self
                .workspaces
                .git(
                    mirror,
                    &["rev-parse", "--verify", "--quiet", &candidate],
                    None,
                )
                .await
            {
                let sha = out.trim().to_string();
                if sha.len() == 40 {
                    return Some(sha);
                }
            }
        }
        None
    }

    /// Export `sha` to its version directory after checking every entry.
    async fn export(&self, mirror: &Path, sha: &str) -> Result<(), String> {
        let dest = self.version_dir(sha);
        if dest.is_dir() {
            return Ok(());
        }
        let listing = self
            .workspaces
            .git(mirror, &["ls-tree", "-r", "-l", "-z", sha], None)
            .await
            .map_err(|e| e.to_string())?;
        let mut files = Vec::new();
        let mut total = 0i64;
        for entry in listing.split('\0').filter(|e| !e.is_empty()) {
            // `<mode> <type> <object> <size>\t<path>`
            let (meta, path) = entry.split_once('\t').ok_or("无法读取文件列表")?;
            let parts: Vec<&str> = meta.split_whitespace().collect();
            let (mode, kind, object, size) = (
                parts[0],
                parts[1],
                parts[2],
                parts.get(3).copied().unwrap_or("0"),
            );
            if mode == "120000" {
                return Err(format!("不允许符号链接：{path}"));
            }
            if kind != "blob" {
                return Err(format!("不允许子模块：{path}"));
            }
            let rel = Path::new(path);
            if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
                return Err(format!("路径无效：{path}"));
            }
            let name = rel
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if forbidden(&name) {
                return Err(format!("仓库中不能有凭据文件：{path}"));
            }
            let size: i64 = size.parse().unwrap_or(0);
            if size > MAX_FILE_BYTES {
                return Err(format!("文件超过 1 MB：{path}"));
            }
            total += size;
            if total > MAX_TOTAL_BYTES {
                return Err("仓库内容超过 20 MB".to_string());
            }
            files.push((path.to_string(), object.to_string()));
        }
        let tmp = self
            .root
            .join(format!(".tmp-{sha}-{}", uuid::Uuid::new_v4().simple()));
        let write = async {
            for (path, object) in &files {
                let bytes = blob(mirror, object).await?;
                if let Some(m) = super::content::find_secret(&String::from_utf8_lossy(&bytes), &[])
                {
                    return Err(format!("{path} 疑似包含凭据（{m}）"));
                }
                let target = tmp.join(path);
                if let Some(parent) = target.parent() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                tokio::fs::write(&target, &bytes)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            tokio::fs::create_dir_all(&tmp)
                .await
                .map_err(|e| e.to_string())?;
            tokio::fs::rename(&tmp, &dest)
                .await
                .map_err(|e| e.to_string())
        }
        .await;
        if write.is_err() {
            let _ = tokio::fs::remove_dir_all(&tmp).await;
        }
        write
    }

    /// Remove exports that are neither kept nor among the newest few.
    fn prune(&self, keep: &BTreeSet<String>) {
        let versions = self.versions();
        let recent: BTreeSet<&String> = versions.iter().rev().take(KEEP_VERSIONS).collect();
        for v in &versions {
            if !keep.contains(v) && !recent.contains(v) {
                let _ = std::fs::remove_dir_all(self.version_dir(v));
            }
        }
    }

    /// Periodic sync in the background.
    pub fn spawn(self, repo: SqliteRepository) {
        tokio::spawn(async move {
            loop {
                let interval = match self.settings(&repo).await {
                    Ok(st) => {
                        if st.url.is_some() {
                            if let Err(e) = self.sync(&repo).await {
                                tracing::warn!("organization rules sync error: {e}");
                            }
                        }
                        st.interval_secs
                    }
                    Err(_) => 300,
                };
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            }
        });
    }
}

/// Raw bytes of a blob in a bare repository.
async fn blob(mirror: &Path, object: &str) -> Result<Vec<u8>, String> {
    let out = tokio::process::Command::new("git")
        .arg("--git-dir")
        .arg(mirror)
        .args(["cat-file", "blob", object])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| format!("git cat-file: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git cat-file failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_refs_are_validated() {
        assert!(valid_url("https://gitlab.com/org/rules.git"));
        assert!(valid_url("git@github.com:org/rules.git"));
        assert!(valid_url("/srv/git/rules.git"));
        assert!(!valid_url("--upload-pack=touch /tmp/x"));
        assert!(!valid_url("file:///etc"));
        assert!(!valid_url("https://a b"));
        assert!(!valid_url("-"));
        assert!(valid_ref("main") && valid_ref("release/2026-11") && valid_ref("v1.2.0"));
        assert!(!valid_ref("-x") && !valid_ref("a..b") && !valid_ref("a b") && !valid_ref(""));
    }

    #[test]
    fn credential_file_names_are_forbidden() {
        for n in [
            "auth.json",
            ".env",
            ".env.local",
            "server.pem",
            "x.KEY",
            "id_rsa",
        ] {
            assert!(forbidden(n), "{n}");
        }
        for n in ["AGENTS.md", "config.toml", "env.md", "keys.md"] {
            assert!(!forbidden(n), "{n}");
        }
    }
}
