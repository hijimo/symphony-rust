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
    /// When each signal was last notified (S5 throttle).
    pub notified: std::sync::Arc<dashmap::DashMap<i64, std::time::Instant>>,
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
    Created {
        id: i64,
    },
    /// `linked_issue`: the signal already produced an intent for this issue.
    Updated {
        id: i64,
        linked_issue: Option<i64>,
    },
    /// New fingerprint dropped: too many untriaged signals.
    Dropped,
}

impl SignalStore {
    pub fn new(pool: DbPool) -> Self {
        Self {
            pool,
            notified: Default::default(),
        }
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
            let existing: Option<(i64, String, Option<i64>)> = tx
                .query_row(
                    "SELECT id, status, issue_iid FROM signals WHERE project_id = ?1 AND source = ?2 AND fingerprint = ?3",
                    params![project_id, source, s.fingerprint],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let out = match existing {
                Some((id, status, issue_iid)) => {
                    // An ignore period that has passed ends with this trigger.
                    tx.execute(
                        "UPDATE signals SET status = 'new', ignored_until = NULL
                         WHERE id = ?1 AND status = 'ignored'
                           AND ignored_until IS NOT NULL AND ignored_until <= datetime('now')",
                        params![id],
                    )?;
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
                    Upsert::Updated {
                        id,
                        linked_issue: issue_iid.filter(|_| status == "intent"),
                    }
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
                        Upsert::Created {
                            id: tx.last_insert_rowid(),
                        }
                    }
                }
            };
            tx.commit()?;
            Ok(out)
        })
        .await
    }

    /// Untriaged signals for the board: severity first, then most recent.
    pub async fn pending(&self, project_id: i64, limit: i64) -> Result<Vec<Signal>> {
        blocking(self.pool.clone(), move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {SIGNAL_COLS} FROM signals WHERE project_id = ?1 AND status = 'new'
                 ORDER BY CASE severity WHEN 'critical' THEN 0 WHEN 'warning' THEN 1 ELSE 2 END,
                          last_seen_at DESC, id DESC
                 LIMIT ?2"
            ))?;
            let rows = stmt.query_map(params![project_id, limit], row_signal)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
        .await
    }

    pub async fn get(&self, project_id: i64, id: i64) -> Result<Option<Signal>> {
        blocking(self.pool.clone(), move |c| {
            Ok(c.query_row(
                &format!("SELECT {SIGNAL_COLS} FROM signals WHERE id = ?1 AND project_id = ?2"),
                params![id, project_id],
                row_signal,
            )
            .optional()?)
        })
        .await
    }

    /// Atomically move a `new` signal to `intent` (prevents double starts).
    /// `None` when it is not untriaged (or does not exist).
    pub async fn claim_for_intent(
        &self,
        project_id: i64,
        id: i64,
        user_id: i64,
    ) -> Result<Option<Signal>> {
        let claimed = blocking(self.pool.clone(), move |c| {
            Ok(c.execute(
                "UPDATE signals SET status = 'intent', triaged_by = ?3, triaged_at = datetime('now')
                 WHERE id = ?1 AND project_id = ?2 AND status = 'new'",
                params![id, project_id, user_id],
            )? > 0)
        })
        .await?;
        if claimed {
            self.get(project_id, id).await
        } else {
            Ok(None)
        }
    }

    /// Undo [`claim_for_intent`] after a failed start.
    pub async fn release_claim(&self, id: i64) -> Result<()> {
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "UPDATE signals SET status = 'new', triaged_by = NULL, triaged_at = NULL WHERE id = ?1 AND status = 'intent'",
                params![id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn link_intent(&self, id: i64, flow_id: i64, issue_iid: i64) -> Result<()> {
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "UPDATE signals SET flow_id = ?2, issue_iid = ?3 WHERE id = ?1",
                params![id, flow_id, issue_iid],
            )?;
            Ok(())
        })
        .await
    }

    /// The intent's issue was closed and the signal fired again: back to
    /// triage. The previous issue stays linked for the card.
    pub async fn mark_recurred(&self, id: i64) -> Result<()> {
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "UPDATE signals SET status = 'new', triaged_by = NULL, triaged_at = NULL WHERE id = ?1 AND status = 'intent'",
                params![id],
            )?;
            Ok(())
        })
        .await
    }

    /// Ignore a signal for `days` (None = permanently).
    pub async fn ignore(
        &self,
        project_id: i64,
        id: i64,
        days: Option<i64>,
        user_id: i64,
    ) -> Result<bool> {
        blocking(self.pool.clone(), move |c| {
            Ok(c.execute(
                "UPDATE signals SET status = 'ignored',
                   ignored_until = CASE WHEN ?3 IS NULL THEN NULL ELSE datetime('now', '+' || ?3 || ' days') END,
                   triaged_by = ?4, triaged_at = datetime('now')
                 WHERE id = ?1 AND project_id = ?2",
                params![id, project_id, days, user_id],
            )? > 0)
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
        assert!(matches!(
            s.upsert(p, "generic", sig("a"), 2).await.unwrap(),
            Upsert::Created { .. }
        ));
        assert!(matches!(
            s.upsert(p, "generic", sig("b"), 2).await.unwrap(),
            Upsert::Created { .. }
        ));
        assert_eq!(
            s.upsert(p, "generic", sig("c"), 2).await.unwrap(),
            Upsert::Dropped
        );
        assert!(
            matches!(
                s.upsert(p, "generic", sig("a"), 2).await.unwrap(),
                Upsert::Updated {
                    linked_issue: None,
                    ..
                }
            ),
            "known fingerprints still update"
        );
        assert!(
            matches!(
                s.upsert(p, "sentry", sig("a"), 3).await.unwrap(),
                Upsert::Created { .. }
            ),
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

    #[tokio::test]
    async fn intent_claims_are_exclusive_and_reversible() {
        let (_d, s, p) = store().await;
        s.upsert(p, "generic", sig("a"), 10).await.unwrap();
        let id = s.list(p, None).await.unwrap()[0].id;
        let claimed = s.claim_for_intent(p, id, 900).await.unwrap().unwrap();
        assert_eq!(
            (claimed.status.as_str(), claimed.triaged_by),
            ("intent", Some(900))
        );
        assert!(
            s.claim_for_intent(p, id, 900).await.unwrap().is_none(),
            "only once"
        );
        assert!(
            s.claim_for_intent(p + 1, id, 900).await.unwrap().is_none(),
            "other project"
        );

        s.release_claim(id).await.unwrap();
        let back = s.get(p, id).await.unwrap().unwrap();
        assert_eq!((back.status.as_str(), back.triaged_by), ("new", None));

        s.claim_for_intent(p, id, 900).await.unwrap().unwrap();
        let flow_id = {
            let c = s.pool.get().unwrap();
            c.execute(
                "INSERT INTO change_flows (project_id, issue_iid, track, branch, created_by) VALUES (?1, 31, 'full', 'b', 900)",
                params![p],
            )
            .unwrap();
            c.last_insert_rowid()
        };
        s.link_intent(id, flow_id, 31).await.unwrap();
        let upd = s.upsert(p, "generic", sig("a"), 10).await.unwrap();
        assert!(
            matches!(
                upd,
                Upsert::Updated {
                    linked_issue: Some(31),
                    ..
                }
            ),
            "{upd:?}"
        );
        s.mark_recurred(id).await.unwrap();
        let rec = s.get(p, id).await.unwrap().unwrap();
        assert_eq!((rec.status.as_str(), rec.issue_iid), ("new", Some(31)));
        assert!(
            matches!(
                s.upsert(p, "generic", sig("a"), 10).await.unwrap(),
                Upsert::Updated {
                    linked_issue: None,
                    ..
                }
            ),
            "recurred signals are untriaged again"
        );
        assert!(s.get(p + 1, id).await.unwrap().is_none());
    }
}
