//! Document-flow metrics (docs/方案/ai-native-sdlc-p2-design.md §4 B5).

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Duration, NaiveDateTime, Utc};
use serde_json::{json, Value};

use super::model::{DocType, FlowState};
use super::store::{FirstApproval, FlowEvent};
use crate::error::WebPlatformError;
use crate::models::Project;
use crate::services::git_platform::ListIssuesOptions;
use crate::AppState;

/// Upper bound of `Rework` label-history lookups per request.
const MAX_REWORK_LOOKUPS: usize = 50;
/// Spec changes that count as requirement rework.
const REWORK_KINDS: [&str; 3] = ["edited", "cascade_stale", "revision_started"];

const SQL_TS: &str = "%Y-%m-%d %H:%M:%S";

fn parse_sql_ts(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, SQL_TS).ok()
}

fn rate(num: i64, den: i64) -> Value {
    if den == 0 {
        Value::Null
    } else {
        json!(((num as f64 / den as f64) * 1000.0).round() / 1000.0)
    }
}

/// Nearest-rank percentile of sorted values.
pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.clamp(1, sorted.len()) - 1])
}

/// Hours from the first G1 approval to the first G2 approval, for flows
/// whose G2 approval falls in the window.
pub fn design_cycle_hours(first: &[FirstApproval], since: &str) -> Vec<f64> {
    let mut by_flow: HashMap<i64, HashMap<&str, &str>> = HashMap::new();
    for a in first {
        by_flow
            .entry(a.flow_id)
            .or_default()
            .insert(a.gate.as_str(), a.created_at.as_str());
    }
    let mut out: Vec<f64> = by_flow
        .values()
        .filter_map(|g| {
            let g2 = *g.get("G2")?;
            if g2 < since {
                return None;
            }
            let (g1, g2) = (parse_sql_ts(g.get("G1")?)?, parse_sql_ts(g2)?);
            Some(((g2 - g1).num_seconds().max(0) as f64) / 3600.0)
        })
        .collect();
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Spec edits / stale cascades after the plan was first submitted, within
/// the window: `(events, flows)`.
pub fn rework(events: &[FlowEvent], since: &str) -> (i64, i64) {
    let mut plan_submitted: HashMap<i64, &str> = HashMap::new();
    let mut count = 0;
    let mut flows = HashSet::new();
    for e in events {
        if e.kind == "submitted" && e.doc_type.as_deref() == Some("plan") {
            plan_submitted
                .entry(e.flow_id)
                .or_insert(e.created_at.as_str());
            continue;
        }
        let after_plan = plan_submitted
            .get(&e.flow_id)
            .is_some_and(|t| e.created_at.as_str() >= *t);
        if after_plan
            && e.created_at.as_str() >= since
            && e.doc_type.as_deref() == Some("spec")
            && REWORK_KINDS.contains(&e.kind.as_str())
        {
            count += 1;
            flows.insert(e.flow_id);
        }
    }
    (count, flows.len() as i64)
}

/// Whether the issue ever had `Rework`; closed issues are cached for good.
async fn had_rework(
    client: &dyn crate::services::git_platform::GitPlatformClient,
    token: &str,
    path: &str,
    project_id: i64,
    iid: u64,
) -> Option<bool> {
    static CACHE: std::sync::OnceLock<dashmap::DashMap<(i64, u64), bool>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(dashmap::DashMap::new);
    if let Some(v) = cache.get(&(project_id, iid)) {
        return Some(*v);
    }
    let v = client
        .label_added_at(token, path, iid, "Rework")
        .await
        .ok()?
        .is_some();
    cache.insert((project_id, iid), v);
    Some(v)
}

/// Closed-issue metrics from the platform (first-pass merge, plan
/// conformance).
async fn closed_issue_metrics(
    app: &AppState,
    project: &Project,
    since: chrono::DateTime<Utc>,
) -> Result<(Value, Value), WebPlatformError> {
    let s = &app.sdlc;
    let token = super::service::project_token(app, project).await?;
    let client = super::service::client(app, project).await?;
    let path = super::service::project_path(project);
    let (closed, _) = client
        .list_issues(
            &token,
            &path,
            &ListIssuesOptions {
                state: Some("closed".to_string()),
                limit: 100,
                ..Default::default()
            },
        )
        .await
        .map_err(|e| WebPlatformError::ExternalService(e.to_string()))?;
    let recent: Vec<_> = closed
        .into_iter()
        .filter(|i| {
            i.closed_at
                .as_deref()
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                .is_some_and(|t| t.with_timezone(&Utc) >= since)
        })
        .collect();

    // iid → (rejected, has plan)
    let mut flows: HashMap<u64, (bool, bool)> = HashMap::new();
    for f in s.store.list_flows(project.id).await? {
        let docs = s.store.list_documents(f.id).await?;
        let statuses: BTreeMap<_, _> = docs.iter().map(|d| (d.doc_type, d.status)).collect();
        let st = super::state::derive_flow_state(f.track, &statuses);
        flows.insert(
            f.issue_iid as u64,
            (
                st == FlowState::Rejected,
                docs.iter().any(|d| d.doc_type == DocType::Plan),
            ),
        );
    }

    let mut closed_n = 0;
    let mut first_pass = 0;
    let mut partial = false;
    for (n, i) in recent
        .iter()
        .filter(|i| !flows.get(&i.iid).is_some_and(|f| f.0))
        .enumerate()
    {
        if n >= MAX_REWORK_LOOKUPS {
            partial = true;
            break;
        }
        match had_rework(client.as_ref(), &token, &path, project.id, i.iid).await {
            Some(rework) => {
                closed_n += 1;
                if !rework {
                    first_pass += 1;
                }
            }
            None => partial = true,
        }
    }
    let with_plan: Vec<_> = recent
        .iter()
        .filter(|i| flows.get(&i.iid).is_some_and(|f| !f.0 && f.1))
        .collect();
    let conforming = with_plan
        .iter()
        .filter(|i| !i.labels.iter().any(|l| l == "plan-deviation"))
        .count() as i64;
    Ok((
        json!({"closed": closed_n, "firstPass": first_pass, "rate": rate(first_pass, closed_n), "partial": partial}),
        json!({"closed": with_plan.len(), "conforming": conforming, "rate": rate(conforming, with_plan.len() as i64)}),
    ))
}

/// All metrics of a project over the last `days` days.
pub async fn project_metrics(
    app: &AppState,
    project: &Project,
    days: i64,
) -> Result<Value, WebPlatformError> {
    let s = &app.sdlc;
    let now = Utc::now();
    let since = now - Duration::days(days);
    let since_sql = since.format(SQL_TS).to_string();

    let accepted = s
        .store
        .count_gate_decisions(project.id, "G1", "approve", since_sql.clone())
        .await?;
    let rejected = s
        .store
        .count_gate_decisions(project.id, "G1", "reject", since_sql.clone())
        .await?;
    let cycle = design_cycle_hours(&s.store.first_approvals(project.id).await?, &since_sql);
    let (rework_n, rework_flows) =
        rework(&s.store.project_flow_events(project.id).await?, &since_sql);
    let tokens = s
        .store
        .project_token_usage(project.id, since.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .await?;
    let (first_pass, conformance) = match closed_issue_metrics(app, project, since).await {
        Ok(v) => v,
        Err(e) => {
            let err = json!({"rate": null, "error": e.to_string()});
            (err.clone(), err)
        }
    };
    let round = |v: Option<f64>| v.map(|h| (h * 10.0).round() / 10.0);
    Ok(json!({
        "days": days,
        "intentSurvival": {"accepted": accepted, "rejected": rejected, "rate": rate(accepted, accepted + rejected)},
        "designCycle": {
            "count": cycle.len(),
            "medianHours": round(percentile(&cycle, 50.0)),
            "p90Hours": round(percentile(&cycle, 90.0)),
        },
        "rework": {"count": rework_n, "flows": rework_flows},
        "firstPassMerge": first_pass,
        "planConformance": conformance,
        "draftTokens": tokens,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(flow: i64, kind: &str, doc: &str, at: &str) -> FlowEvent {
        FlowEvent {
            flow_id: flow,
            kind: kind.into(),
            doc_type: Some(doc.into()),
            created_at: at.into(),
        }
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        assert_eq!(percentile(&[], 50.0), None);
        assert_eq!(percentile(&[4.0], 90.0), Some(4.0));
        let v: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(percentile(&v, 50.0), Some(5.0));
        assert_eq!(percentile(&v, 90.0), Some(9.0));
    }

    #[test]
    fn design_cycle_needs_both_gates_and_g2_in_window() {
        let a = |f, g: &str, t: &str| FirstApproval {
            flow_id: f,
            gate: g.into(),
            created_at: t.into(),
        };
        let hours = design_cycle_hours(
            &[
                a(1, "G1", "2026-11-01 00:00:00"),
                a(1, "G2", "2026-11-01 06:00:00"),
                a(2, "G1", "2026-10-01 00:00:00"),
                a(2, "G2", "2026-10-02 00:00:00"),
                a(3, "G1", "2026-11-02 00:00:00"),
                a(4, "G2", "2026-11-02 00:00:00"),
            ],
            "2026-10-15 00:00:00",
        );
        assert_eq!(hours, vec![6.0]);
    }

    #[test]
    fn rework_counts_spec_changes_after_plan_submission() {
        let events = [
            ev(1, "edited", "spec", "2026-11-01 00:00:00"),
            ev(1, "submitted", "plan", "2026-11-02 00:00:00"),
            ev(1, "edited", "spec", "2026-11-03 00:00:00"),
            ev(1, "cascade_stale", "spec", "2026-11-04 00:00:00"),
            ev(1, "edited", "plan", "2026-11-04 00:00:00"),
            ev(1, "docs_merged", "spec", "2026-11-05 00:00:00"),
            ev(2, "edited", "spec", "2026-11-03 00:00:00"),
            ev(3, "submitted", "plan", "2026-09-01 00:00:00"),
            ev(3, "revision_started", "spec", "2026-09-02 00:00:00"),
        ];
        assert_eq!(rework(&events, "2026-10-01 00:00:00"), (2, 1));
    }

    #[test]
    fn rates_are_null_without_samples() {
        assert_eq!(rate(0, 0), Value::Null);
        assert_eq!(rate(1, 3), json!(0.333));
    }
}
