//! Persistence for change flows, documents, approvals and agent sessions
//! (§4.6). Uses the shared SQLite pool; every call runs in spawn_blocking.

use std::collections::BTreeMap;

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::model::{DocStatus, DocType, Track};
use crate::db::DbPool;
use crate::error::{Result, WebPlatformError};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeFlow {
    pub id: i64,
    pub project_id: i64,
    pub issue_iid: i64,
    pub track: Track,
    pub revision: i64,
    pub branch: String,
    pub docs_mr_iid: Option<i64>,
    pub docs_mr_url: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub merge_error: Option<String>,
    pub base_sha: Option<String>,
    pub closed: bool,
    pub created_by: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeDocument {
    pub id: i64,
    pub flow_id: i64,
    pub doc_type: DocType,
    pub status: DocStatus,
    pub path: String,
    pub current_blob_sha: Option<String>,
    pub template_ref: Option<String>,
    pub org_config_sha: Option<String>,
    pub base_sha: Option<String>,
    pub draft_state: String,
    pub draft_failures: i64,
    pub draft_error: Option<String>,
    pub last_feedback: Option<String>,
    pub updated_by: Option<i64>,
    pub updated_at: String,
    /// Last status transition (UTC, `YYYY-MM-DD HH:MM:SS`).
    pub status_changed_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentApproval {
    pub id: i64,
    pub document_id: i64,
    pub gate: String,
    pub action: String,
    pub actor: i64,
    pub actor_name: Option<String>,
    pub comment: Option<String>,
    pub blob_sha: String,
    pub commit_sha: Option<String>,
    pub created_at: String,
    pub invalidated_at: Option<String>,
}

#[derive(Clone)]
pub struct SdlcStore {
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

const FLOW_COLS: &str = "id, project_id, issue_iid, track, revision, branch, docs_mr_iid, docs_mr_url, merge_commit_sha, merge_error, base_sha, closed, created_by, created_at, updated_at";
const DOC_COLS: &str = "id, flow_id, doc_type, status, path, current_blob_sha, template_ref, org_config_sha, base_sha, draft_state, draft_failures, draft_error, last_feedback, updated_by, updated_at, COALESCE(status_changed_at, updated_at)";

fn row_flow(r: &rusqlite::Row) -> rusqlite::Result<(ChangeFlow, String)> {
    let track: String = r.get(3)?;
    Ok((
        ChangeFlow {
            id: r.get(0)?,
            project_id: r.get(1)?,
            issue_iid: r.get(2)?,
            track: Track::Plan,
            revision: r.get(4)?,
            branch: r.get(5)?,
            docs_mr_iid: r.get(6)?,
            docs_mr_url: r.get(7)?,
            merge_commit_sha: r.get(8)?,
            merge_error: r.get(9)?,
            base_sha: r.get(10)?,
            closed: r.get::<_, i64>(11)? != 0,
            created_by: r.get(12)?,
            created_at: r.get(13)?,
            updated_at: r.get(14)?,
        },
        track,
    ))
}

fn finish_flow((mut f, track): (ChangeFlow, String)) -> Result<ChangeFlow> {
    f.track = Track::parse(&track)
        .map_err(|_| WebPlatformError::Internal(format!("bad track in db: {track}")))?;
    Ok(f)
}

fn row_doc(r: &rusqlite::Row) -> rusqlite::Result<(ChangeDocument, String, String)> {
    let doc_type: String = r.get(2)?;
    let status: String = r.get(3)?;
    Ok((
        ChangeDocument {
            id: r.get(0)?,
            flow_id: r.get(1)?,
            doc_type: DocType::Plan,
            status: DocStatus::Draft,
            path: r.get(4)?,
            current_blob_sha: r.get(5)?,
            template_ref: r.get(6)?,
            org_config_sha: r.get(7)?,
            base_sha: r.get(8)?,
            draft_state: r.get(9)?,
            draft_failures: r.get(10)?,
            draft_error: r.get(11)?,
            last_feedback: r.get(12)?,
            updated_by: r.get(13)?,
            updated_at: r.get(14)?,
            status_changed_at: r.get(15)?,
        },
        doc_type,
        status,
    ))
}

fn finish_doc((mut d, t, s): (ChangeDocument, String, String)) -> Result<ChangeDocument> {
    d.doc_type = t
        .parse()
        .map_err(|_| WebPlatformError::Internal(format!("bad doc_type in db: {t}")))?;
    d.status = DocStatus::parse(&s)?;
    Ok(d)
}

/// New document row.
pub struct NewDocument {
    pub doc_type: DocType,
    pub path: String,
    pub blob_sha: Option<String>,
    pub template_ref: String,
    pub base_sha: Option<String>,
    pub updated_by: i64,
}

/// Approval to record.
pub struct NewApproval<'a> {
    pub document_id: i64,
    pub gate: &'a str,
    pub action: &'a str,
    pub actor: i64,
    pub comment: Option<&'a str>,
    pub blob_sha: &'a str,
    pub commit_sha: Option<&'a str>,
}

impl SdlcStore {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn create_flow(
        &self,
        project_id: i64,
        issue_iid: i64,
        track: Track,
        branch: &str,
        created_by: i64,
    ) -> Result<ChangeFlow> {
        let branch = branch.to_string();
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "INSERT INTO change_flows (project_id, issue_iid, track, branch, created_by) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![project_id, issue_iid, track.as_str(), branch, created_by],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(ref err, _)
                    if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    WebPlatformError::Conflict("该 Issue 已经开始文档流程".to_string())
                }
                other => other.into(),
            })?;
            let id = c.last_insert_rowid();
            let row = c.query_row(
                &format!("SELECT {FLOW_COLS} FROM change_flows WHERE id = ?1"),
                params![id],
                row_flow,
            )?;
            finish_flow(row)
        })
        .await
    }

    pub async fn get_flow(&self, project_id: i64, issue_iid: i64) -> Result<Option<ChangeFlow>> {
        blocking(self.pool.clone(), move |c| {
            let row = c
                .query_row(
                    &format!("SELECT {FLOW_COLS} FROM change_flows WHERE project_id = ?1 AND issue_iid = ?2"),
                    params![project_id, issue_iid],
                    row_flow,
                )
                .optional()?;
            row.map(finish_flow).transpose()
        })
        .await
    }

    pub async fn get_flow_by_id(&self, flow_id: i64) -> Result<Option<ChangeFlow>> {
        blocking(self.pool.clone(), move |c| {
            let row = c
                .query_row(
                    &format!("SELECT {FLOW_COLS} FROM change_flows WHERE id = ?1"),
                    params![flow_id],
                    row_flow,
                )
                .optional()?;
            row.map(finish_flow).transpose()
        })
        .await
    }

    pub async fn list_flows(&self, project_id: i64) -> Result<Vec<ChangeFlow>> {
        blocking(self.pool.clone(), move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {FLOW_COLS} FROM change_flows WHERE project_id = ?1 ORDER BY issue_iid DESC"
            ))?;
            let rows = stmt.query_map(params![project_id], row_flow)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(finish_flow(r?)?);
            }
            Ok(out)
        })
        .await
    }

    pub async fn update_flow_mr(
        &self,
        flow_id: i64,
        mr_iid: Option<i64>,
        mr_url: Option<&str>,
    ) -> Result<()> {
        let mr_url = mr_url.map(str::to_string);
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "UPDATE change_flows SET docs_mr_iid = ?2, docs_mr_url = ?3, updated_at = datetime('now') WHERE id = ?1",
                params![flow_id, mr_iid, mr_url],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn set_flow_merge_result(
        &self,
        flow_id: i64,
        merge_commit_sha: Option<&str>,
        merge_error: Option<&str>,
    ) -> Result<()> {
        let sha = merge_commit_sha.map(str::to_string);
        let err = merge_error.map(str::to_string);
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "UPDATE change_flows SET merge_commit_sha = COALESCE(?2, merge_commit_sha), merge_error = ?3, updated_at = datetime('now') WHERE id = ?1",
                params![flow_id, sha, err],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn set_flow_base_sha(&self, flow_id: i64, base_sha: &str) -> Result<()> {
        let base_sha = base_sha.to_string();
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "UPDATE change_flows SET base_sha = ?2, updated_at = datetime('now') WHERE id = ?1",
                params![flow_id, base_sha],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn set_flow_closed(&self, flow_id: i64) -> Result<()> {
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "UPDATE change_flows SET closed = 1, updated_at = datetime('now') WHERE id = ?1",
                params![flow_id],
            )?;
            Ok(())
        })
        .await
    }

    /// Start a new revision (r2…): new branch, MR cleared, documents back to
    /// `in_review` for the chosen gate and downstream; upstream stays approved.
    pub async fn start_revision(
        &self,
        flow_id: i64,
        branch: &str,
        from: DocType,
    ) -> Result<ChangeFlow> {
        let branch = branch.to_string();
        blocking(self.pool.clone(), move |c| {
            let tx = c.unchecked_transaction()?;
            tx.execute(
                "UPDATE change_flows SET revision = revision + 1, branch = ?2, docs_mr_iid = NULL, docs_mr_url = NULL, merge_error = NULL, updated_at = datetime('now') WHERE id = ?1",
                params![flow_id, branch],
            )?;
            let mut stmt = tx.prepare("SELECT id, doc_type FROM change_documents WHERE flow_id = ?1")?;
            let docs: Vec<(i64, String)> = stmt
                .query_map(params![flow_id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            drop(stmt);
            for (id, t) in docs {
                let d: DocType = t.parse()?;
                let status = if d == from {
                    DocStatus::InReview
                } else if from.downstream().contains(&d) {
                    DocStatus::Stale
                } else {
                    DocStatus::Approved
                };
                tx.execute(
                    "UPDATE change_documents SET status = ?2, updated_at = datetime('now'), status_changed_at = datetime('now') WHERE id = ?1",
                    params![id, status.as_str()],
                )?;
            }
            let row = tx.query_row(
                &format!("SELECT {FLOW_COLS} FROM change_flows WHERE id = ?1"),
                params![flow_id],
                row_flow,
            )?;
            tx.commit()?;
            finish_flow(row)
        })
        .await
    }

    pub async fn insert_document(&self, flow_id: i64, d: NewDocument) -> Result<ChangeDocument> {
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "INSERT INTO change_documents (flow_id, doc_type, status, path, current_blob_sha, template_ref, base_sha, updated_by, status_changed_at) VALUES (?1, ?2, 'draft', ?3, ?4, ?5, ?6, ?7, datetime('now'))",
                params![flow_id, d.doc_type.as_str(), d.path, d.blob_sha, d.template_ref, d.base_sha, d.updated_by],
            )?;
            let id = c.last_insert_rowid();
            let row = c.query_row(
                &format!("SELECT {DOC_COLS} FROM change_documents WHERE id = ?1"),
                params![id],
                row_doc,
            )?;
            finish_doc(row)
        })
        .await
    }

    pub async fn list_documents(&self, flow_id: i64) -> Result<Vec<ChangeDocument>> {
        blocking(self.pool.clone(), move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {DOC_COLS} FROM change_documents WHERE flow_id = ?1 ORDER BY id"
            ))?;
            let rows = stmt.query_map(params![flow_id], row_doc)?;
            let mut out = Vec::new();
            for r in rows {
                out.push(finish_doc(r?)?);
            }
            out.sort_by_key(|d| d.doc_type);
            Ok(out)
        })
        .await
    }

    pub async fn get_document(&self, flow_id: i64, doc: DocType) -> Result<Option<ChangeDocument>> {
        blocking(self.pool.clone(), move |c| {
            let row = c
                .query_row(
                    &format!("SELECT {DOC_COLS} FROM change_documents WHERE flow_id = ?1 AND doc_type = ?2"),
                    params![flow_id, doc.as_str()],
                    row_doc,
                )
                .optional()?;
            row.map(finish_doc).transpose()
        })
        .await
    }

    pub async fn statuses(&self, flow_id: i64) -> Result<BTreeMap<DocType, DocStatus>> {
        Ok(self
            .list_documents(flow_id)
            .await?
            .into_iter()
            .map(|d| (d.doc_type, d.status))
            .collect())
    }

    pub async fn set_status(&self, doc_id: i64, status: DocStatus, by: Option<i64>) -> Result<()> {
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "UPDATE change_documents SET status = ?2, updated_by = COALESCE(?3, updated_by), updated_at = datetime('now'), status_changed_at = CASE WHEN status = ?2 THEN status_changed_at ELSE datetime('now') END WHERE id = ?1",
                params![doc_id, status.as_str(), by],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn set_blob(&self, doc_id: i64, blob_sha: &str, by: Option<i64>) -> Result<()> {
        let blob_sha = blob_sha.to_string();
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "UPDATE change_documents SET current_blob_sha = ?2, updated_by = COALESCE(?3, updated_by), updated_at = datetime('now') WHERE id = ?1",
                params![doc_id, blob_sha, by],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn set_feedback(&self, doc_id: i64, feedback: Option<&str>) -> Result<()> {
        let feedback = feedback.map(str::to_string);
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "UPDATE change_documents SET last_feedback = ?2 WHERE id = ?1",
                params![doc_id, feedback],
            )?;
            Ok(())
        })
        .await
    }

    /// Draft state: `running` resets nothing; `idle` resets failures; `failed`
    /// increments the failure counter.
    pub async fn set_draft_state(
        &self,
        doc_id: i64,
        state: &str,
        error: Option<&str>,
    ) -> Result<i64> {
        let state = state.to_string();
        let error = error.map(str::to_string);
        blocking(self.pool.clone(), move |c| {
            match state.as_str() {
                "failed" => c.execute(
                    "UPDATE change_documents SET draft_state = 'failed', draft_failures = draft_failures + 1, draft_error = ?2 WHERE id = ?1",
                    params![doc_id, error],
                )?,
                "idle" => c.execute(
                    "UPDATE change_documents SET draft_state = 'idle', draft_failures = 0, draft_error = NULL WHERE id = ?1",
                    params![doc_id],
                )?,
                _ => c.execute(
                    "UPDATE change_documents SET draft_state = ?2 WHERE id = ?1",
                    params![doc_id, state],
                )?,
            };
            let failures: i64 = c.query_row(
                "SELECT draft_failures FROM change_documents WHERE id = ?1",
                params![doc_id],
                |r| r.get(0),
            )?;
            Ok(failures)
        })
        .await
    }

    /// Drafts left `running` by a previous process can never finish: mark
    /// them failed so the UI offers retry / manual (review r2 I1).
    pub async fn reset_stale_drafts(&self) -> Result<usize> {
        blocking(self.pool.clone(), move |c| {
            Ok(c.execute(
                "UPDATE change_documents SET draft_state = 'failed', draft_error = '服务重启，起草被中断' WHERE draft_state = 'running'",
                [],
            )?)
        })
        .await
    }

    pub async fn insert_approval(&self, a: NewApproval<'_>) -> Result<()> {
        let (doc, gate, action, actor) = (
            a.document_id,
            a.gate.to_string(),
            a.action.to_string(),
            a.actor,
        );
        let comment = a.comment.map(str::to_string);
        let blob = a.blob_sha.to_string();
        let commit = a.commit_sha.map(str::to_string);
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "INSERT INTO document_approvals (document_id, gate, action, actor, comment, blob_sha, commit_sha) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![doc, gate, action, actor, comment, blob, commit],
            )?;
            Ok(())
        })
        .await
    }

    /// Void still-valid approvals of a document (content changed / stale).
    pub async fn invalidate_approvals(&self, doc_id: i64) -> Result<()> {
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "UPDATE document_approvals SET invalidated_at = datetime('now') WHERE document_id = ?1 AND action = 'approve' AND invalidated_at IS NULL",
                params![doc_id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn list_approvals(&self, flow_id: i64) -> Result<Vec<DocumentApproval>> {
        blocking(self.pool.clone(), move |c| {
            let mut stmt = c.prepare(
                "SELECT a.id, a.document_id, a.gate, a.action, a.actor, u.username, a.comment, a.blob_sha, a.commit_sha, a.created_at, a.invalidated_at
                 FROM document_approvals a
                 JOIN change_documents d ON d.id = a.document_id
                 LEFT JOIN users u ON u.id = a.actor
                 WHERE d.flow_id = ?1 ORDER BY a.id",
            )?;
            let rows = stmt.query_map(params![flow_id], |r| {
                Ok(DocumentApproval {
                    id: r.get(0)?,
                    document_id: r.get(1)?,
                    gate: r.get(2)?,
                    action: r.get(3)?,
                    actor: r.get(4)?,
                    actor_name: r.get(5)?,
                    comment: r.get(6)?,
                    blob_sha: r.get(7)?,
                    commit_sha: r.get(8)?,
                    created_at: r.get(9)?,
                    invalidated_at: r.get(10)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    pub async fn get_session_thread(&self, flow_id: i64, stage: DocType) -> Result<Option<String>> {
        blocking(self.pool.clone(), move |c| {
            Ok(c.query_row(
                "SELECT thread_id FROM agent_sessions WHERE flow_id = ?1 AND stage = ?2",
                params![flow_id, stage.as_str()],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
        })
        .await
    }

    pub async fn set_session_thread(
        &self,
        flow_id: i64,
        stage: DocType,
        thread_id: &str,
    ) -> Result<()> {
        let thread_id = thread_id.to_string();
        blocking(self.pool.clone(), move |c| {
            c.execute(
                "INSERT INTO agent_sessions (flow_id, stage, thread_id) VALUES (?1, ?2, ?3)
                 ON CONFLICT(flow_id, stage) DO UPDATE SET thread_id = excluded.thread_id, updated_at = datetime('now')",
                params![flow_id, stage.as_str(), thread_id],
            )?;
            Ok(())
        })
        .await
    }
}

/// Remove a flow (rollback of a failed start).
pub async fn delete_flow(store: &SdlcStore, flow_id: i64) -> Result<()> {
    blocking(store.pool.clone(), move |c| {
        c.execute("DELETE FROM change_flows WHERE id = ?1", params![flow_id])?;
        Ok(())
    })
    .await
}
