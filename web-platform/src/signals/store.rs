//! `signals` and `signal_ingest_keys` (V027).

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use super::IncomingSignal;
use crate::db::DbPool;
use crate::error::{Result, WebPlatformError};

#[derive(Clone)]
pub struct SignalStore {
    pool: DbPool,
}

async fn blocking<T, F>(pool: DbPool, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&rusqlite::Connection) -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        f(&conn)
    })
    .await
    .map_err(|e| WebPlatformError::Internal(format!("db task failed: {e}")))?
}

/// An ingest key without its secret.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestKey {
    pub id: i64,
    pub project_id: i64,
    pub source: String,
    pub name: String,
    #[serde(skip)]
    pub key_hash: String,
    #[serde(skip)]
    pub secret_enc: Option<String>,
    pub created_by: Option<i64>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}

const KEY_COLS: &str =
    "id, project_id, source, name, key_hash, secret_enc, created_by, created_at, last_used_at, revoked_at";

fn row_key(r: &rusqlite::Row) -> rusqlite::Result<IngestKey> {
    Ok(IngestKey {
        id: r.get(0)?,
        project_id: r.get(1)?,
        source: r.get(2)?,
        name: r.get(3)?,
        key_hash: r.get(4)?,
        secret_enc: r.get(5)?,
        created_by: r.get(6)?,
        created_at: r.get(7)?,
        last_used_at: r.get(8)?,
        revoked_at: r.get(9)?,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Signal {
    pub id: i64,
    pub project_id: i64,
    pub source: String,
    pub fingerprint: String,
    pub title: String,
    pub severity: String,
    pub status: String,
    pub resolved: bool,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub count: i64,
    pub evidence: Value,
    pub external_url: Option<String>,
    pub flow_id: Option<i64>,
    pub issue_iid: Option<i64>,
    pub ignored_until: Option<String>,
    pub triaged_by: Option<i64>,
    pub triaged_at: Option<String>,
}

const SIGNAL_COLS: &str = "id, project_id, source, fingerprint, title, severity, status, resolved, first_seen_at, last_seen_at, count, evidence_json, external_url, flow_id, issue_iid, ignored_until, triaged_by, triaged_at";

fn row_signal(r: &rusqlite::Row) -> rusqlite::Result<Signal> {
    let evidence: String = r.get(11)?;
    Ok(Signal {
        id: r.get(0)?,
        project_id: r.get(1)?,
        source: r.get(2)?,
        fingerprint: r.get(3)?,
        title: r.get(4)?,
        severity: r.get(5)?,
        status: r.get(6)?,
        resolved: r.get::<_, i64>(7)? != 0,
        first_seen_at: r.get(8)?,
        last_seen_at: r.get(9)?,
        count: r.get(10)?,
        evidence: serde_json::from_str(&evidence).unwrap_or(Value::Null),
        external_url: r.get(12)?,
        flow_id: r.get(13)?,
        issue_iid: r.get(14)?,
        ignored_until: r.get(15)?,
        triaged_by: r.get(16)?,
        triaged_at: r.get(17)?,
    })
}

/// Result of storing one incoming signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upsert {
    Created,
    Updated,
    /// New fingerprint dropped: too many untriaged signals.
    Dropped,
}

impl SignalStore {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn create_key(
        &self,
        project_id: i64,
        source: &str,
        name: &str,
        key_hash: &str,
        secret_enc: Option<String>,
        created_by: i64,
    ) -> Result<IngestKey> {
        let (source, name, key_hash) = (source.to_string(), name.to_string(), key_hash.to_string());
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "INSERT INTO signal_ingest_keys (project_id, source, name, key_hash, secret_enc, created_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![project_id, source, name, key_hash, secret_enc, created_by],
            )?;
            let id = c.last_insert_rowid();
            Ok(c.query_row(
                &format!("SELECT {KEY_COLS} FROM signal_ingest_keys WHERE id = ?1"),
                params![id],
                row_key,
            )?)
        })
        .await
    }

    pub async fn list_keys(&self, project_id: i64) -> Result<Vec<IngestKey>> {
        blocking(self.pool.clone(), move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {KEY_COLS} FROM signal_ingest_keys WHERE project_id = ?1 ORDER BY id"
            ))?;
            let rows = stmt.query_map(params![project_id], row_key)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
        .await
    }

    /// Active keys of a project for one source.
    pub async fn active_keys(&self, project_id: i64, source: &str) -> Result<Vec<IngestKey>> {
        let source = source.to_string();
        blocking(self.pool.clone(), move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {KEY_COLS} FROM signal_ingest_keys
                 WHERE project_id = ?1 AND source = ?2 AND revoked_at IS NULL"
            ))?;
            let rows = stmt.query_map(params![project_id, source], row_key)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
        .await
    }

    /// `false` when the key does not belong to the project.
    pub async fn revoke_key(&self, project_id: i64, key_id: i64) -> Result<bool> {
        blocking(self.pool.clone(), move |c| {
            Ok(c.execute(
                "UPDATE signal_ingest_keys SET revoked_at = COALESCE(revoked_at, datetime('now'))
                 WHERE id = ?1 AND project_id = ?2",
                params![key_id, project_id],
            )? > 0)
        })
        .await
    }

    pub async fn touch_key(&self, key_id: i64) -> Result<()> {
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "UPDATE signal_ingest_keys SET last_used_at = datetime('now') WHERE id = ?1",
                params![key_id],
            )?;
            Ok(())
        })
        .await
    }

    /// Insert a new fingerprint or refresh an existing one (count, last seen,
    /// latest evidence, resolved). The triage status is left unchanged.
    pub async fn upsert(
        &self,
        project_id: i64,
        source: &str,
        s: IncomingSignal,
        max_pending: i64,
    ) -> Result<Upsert> {
        let source = source.to_string();
        blocking(self.pool.clone(), move |c| {
            let tx = c.unchecked_transaction()?;
            let evidence = s.evidence.to_string();
            let existing: Option<i64> = tx
                .query_row(
                    "SELECT id FROM signals WHERE project_id = ?1 AND source = ?2 AND fingerprint = ?3",
                    params![project_id, source, s.fingerprint],
                    |r| r.get(0),
                )
                .optional()?;
            let out = match existing {
                Some(id) => {
                    tx.execute(
                        "UPDATE signals SET title = ?2, severity = COALESCE(?3, severity), resolved = ?4,
                           evidence_json = ?5, external_url = COALESCE(?6, external_url),
                           last_seen_at = datetime('now'), count = count + 1
                         WHERE id = ?1",
                        params![
                            id,
                            s.title,
                            s.severity.map(|v| v.as_str()),
                            s.resolved as i64,
                            evidence,
                            s.external_url
                        ],
                    )?;
                    Upsert::Updated
                }
                None => {
                    let pending: i64 = tx.query_row(
                        "SELECT COUNT(*) FROM signals WHERE project_id = ?1 AND status = 'new'",
                        params![project_id],
                        |r| r.get(0),
                    )?;
                    if pending >= max_pending {
                        Upsert::Dropped
                    } else {
                        tx.execute(
                            "INSERT INTO signals (project_id, source, fingerprint, title, severity, resolved, evidence_json, external_url)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                            params![
                                project_id,
                                source,
                                s.fingerprint,
                                s.title,
                                s.severity.unwrap_or(super::Severity::Warning).as_str(),
                                s.resolved as i64,
                                evidence,
                                s.external_url
                            ],
                        )?;
                        Upsert::Created
                    }
                }
            };
            tx.commit()?;
            Ok(out)
        })
        .await
    }

    /// Signals of a project, most recent first; `status` filters.
    pub async fn list(&self, project_id: i64, status: Option<String>) -> Result<Vec<Signal>> {
        blocking(self.pool.clone(), move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {SIGNAL_COLS} FROM signals
                 WHERE project_id = ?1 AND (?2 IS NULL OR status = ?2)
                 ORDER BY last_seen_at DESC, id DESC LIMIT 500"
            ))?;
            let rows = stmt.query_map(params![project_id, status], row_signal)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::Severity;
    use serde_json::json;

    async fn store() -> (tempfile::TempDir, SignalStore, i64) {
        let dir = tempfile::tempdir().unwrap();
        let pool = crate::db::init_pool(dir.path().join("t.db").to_str().unwrap());
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO projects (name, git_url, platform, namespace, repo_name) VALUES ('p', 'u', 'gitlab', 'n', 'r')",
            [],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO users (id, username, password_hash) VALUES (900, 'u900', 'x')",
            [],
        )
        .unwrap();
        drop(conn);
        (dir, SignalStore::new(pool), id)
    }

    fn sig(fp: &str) -> IncomingSignal {
        IncomingSignal {
            fingerprint: fp.into(),
            title: format!("t-{fp}"),
            severity: Some(Severity::Info),
            resolved: false,
            evidence: json!({}),
            external_url: None,
        }
    }

    #[tokio::test]
    async fn new_fingerprints_are_dropped_over_the_pending_cap() {
        let (_d, s, p) = store().await;
        assert_eq!(
            s.upsert(p, "generic", sig("a"), 2).await.unwrap(),
            Upsert::Created
        );
        assert_eq!(
            s.upsert(p, "generic", sig("b"), 2).await.unwrap(),
            Upsert::Created
        );
        assert_eq!(
            s.upsert(p, "generic", sig("c"), 2).await.unwrap(),
            Upsert::Dropped
        );
        assert_eq!(
            s.upsert(p, "generic", sig("a"), 2).await.unwrap(),
            Upsert::Updated,
            "known fingerprints still update"
        );
        assert_eq!(
            s.upsert(p, "sentry", sig("a"), 3).await.unwrap(),
            Upsert::Created,
            "fingerprints are per source"
        );
        assert_eq!(s.list(p, Some("new".into())).await.unwrap().len(), 3);
        assert!(s.list(p, Some("ignored".into())).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn keys_are_revoked_per_project() {
        let (_d, s, p) = store().await;
        let k = s
            .create_key(p, "generic", "k", "h", None, 900)
            .await
            .unwrap();
        assert!(!s.revoke_key(p + 1, k.id).await.unwrap());
        assert!(s.revoke_key(p, k.id).await.unwrap());
        assert!(s.active_keys(p, "generic").await.unwrap().is_empty());
        assert!(s.list_keys(p).await.unwrap()[0].revoked_at.is_some());
    }
}
