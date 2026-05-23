//! Reconciler — stall detection and tracker state refresh.
//!
//! Implements SPEC section 8.5: active run reconciliation with two parts:
//! - Part A: Stall detection using monotonic clock
//! - Part B: Tracker state refresh and termination decisions

use std::time::Instant;

use crate::models::{OrchestratorState, normalize_state};

/// Hard deadline: after cancel signal, wait at most 30s before force-killing.
const CANCEL_HARD_DEADLINE_MS: u64 = 30_000;

/// Information about a force-killed entry that needs retry scheduling.
#[derive(Debug)]
pub struct ForceKilledEntry {
    pub issue_id: String,
    pub identifier: String,
    pub attempt: u32,
}

/// Result of tracker state reconciliation for a single issue.
#[derive(Debug)]
pub enum ReconcileAction {
    /// Issue is in a terminal state: terminate worker and clean workspace.
    TerminateAndClean { issue_id: String, identifier: String },
    /// Issue is still active: update the in-memory snapshot.
    UpdateSnapshot { issue_id: String },
    /// Issue is neither active nor terminal: terminate without cleanup.
    TerminateNoClean { issue_id: String, identifier: String },
}

/// Reconcile stalled runs using monotonic clock (SPEC section 8.5 Part A).
///
/// For each running issue:
/// - If already cancelled and past hard deadline: force-kill and collect for retry.
/// - If stall timeout exceeded: send cancel signal and record cancel time.
///
/// Returns entries that were force-killed and need retry scheduling.
/// The caller is responsible for calling schedule_retry on each returned entry.
pub fn reconcile_stalled_runs(
    state: &mut OrchestratorState,
    stall_timeout_ms: i64,
) -> Vec<ForceKilledEntry> {
    // If stall detection is disabled, return immediately
    if stall_timeout_ms <= 0 {
        return Vec::new();
    }

    let mut to_force_kill: Vec<String> = Vec::new();

    // Phase 1: Check for stalls and hard deadline violations
    for (issue_id, entry) in state.running.iter_mut() {
        // Already cancelled: check hard deadline
        if let Some(cancel_time) = entry.cancel_sent_at {
            if cancel_time.elapsed().as_millis() as u64 > CANCEL_HARD_DEADLINE_MS {
                // Worker exceeded hard deadline after cancel, mark for force kill
                to_force_kill.push(issue_id.clone());
            }
            // Already cancelled entries don't get re-checked for stall
            continue;
        }

        // Use monotonic clock to detect stall (avoids NTP drift)
        let elapsed_ms = entry.session.last_activity_instant.elapsed().as_millis() as i64;
        if elapsed_ms > stall_timeout_ms {
            // First stall detection: send cancel signal
            entry.cancel_token.cancel();
            entry.cancel_sent_at = Some(Instant::now());
            tracing::warn!(
                issue_id = %issue_id,
                identifier = %entry.identifier,
                elapsed_ms,
                stall_timeout_ms,
                "stall detected, cancelling worker"
            );
        }
    }

    // Phase 2: Force-kill entries that exceeded hard deadline
    let mut needs_retry = Vec::new();
    for issue_id in to_force_kill {
        if let Some(entry) = state.running.remove(&issue_id) {
            entry.worker_handle.abort();
            tracing::error!(
                issue_id = %issue_id,
                identifier = %entry.identifier,
                "worker exceeded cancel hard deadline, force-killed"
            );
            needs_retry.push(ForceKilledEntry {
                issue_id: issue_id.clone(),
                identifier: entry.identifier.clone(),
                attempt: entry.retry_attempt.unwrap_or(0),
            });
            // Note: claimed is NOT removed here; schedule_retry will maintain it
        }
    }

    needs_retry
}

/// Determine reconciliation actions based on refreshed tracker states (SPEC section 8.5 Part B).
///
/// For each running issue, given its refreshed state from the tracker:
/// - Terminal state -> terminate and clean workspace
/// - Active state -> update in-memory snapshot
/// - Other state (neither active nor terminal) -> terminate without cleanup
///
/// Returns a list of actions for the orchestrator to execute.
pub fn determine_reconcile_actions(
    running_issue_ids: &[String],
    refreshed_states: &[(String, String)], // (issue_id, current_state)
    active_states: &[String],
    terminal_states: &[String],
    identifiers: &[(String, String)], // (issue_id, identifier)
) -> Vec<ReconcileAction> {
    let mut actions = Vec::new();

    for (issue_id, current_state) in refreshed_states {
        // Only process issues that are currently running
        if !running_issue_ids.contains(issue_id) {
            continue;
        }

        let normalized = normalize_state(current_state);
        let is_terminal = terminal_states
            .iter()
            .any(|ts| normalize_state(ts) == normalized);
        let is_active = active_states
            .iter()
            .any(|s| normalize_state(s) == normalized);

        let identifier = identifiers
            .iter()
            .find(|(id, _)| id == issue_id)
            .map(|(_, ident)| ident.clone())
            .unwrap_or_default();

        if is_terminal {
            actions.push(ReconcileAction::TerminateAndClean {
                issue_id: issue_id.clone(),
                identifier,
            });
        } else if is_active {
            actions.push(ReconcileAction::UpdateSnapshot {
                issue_id: issue_id.clone(),
            });
        } else {
            actions.push(ReconcileAction::TerminateNoClean {
                issue_id: issue_id.clone(),
                identifier,
            });
        }
    }

    actions
}

/// Execute a terminate action on a running entry.
/// Cancels the worker and removes it from the running map.
/// Returns the identifier and attempt for potential retry scheduling.
pub fn terminate_running_entry(
    state: &mut OrchestratorState,
    issue_id: &str,
) -> Option<(String, u32)> {
    if let Some(entry) = state.running.remove(issue_id) {
        entry.cancel_token.cancel();
        entry.worker_handle.abort();

        // Update runtime totals
        state.codex_totals.add_runtime(entry.started_at);

        // Remove from claimed
        state.claimed.remove(issue_id);

        tracing::info!(
            issue_id,
            identifier = %entry.identifier,
            "terminated running entry via reconciliation"
        );

        Some((entry.identifier, entry.retry_attempt.unwrap_or(0)))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    

    #[test]
    fn test_determine_reconcile_actions_terminal() {
        let running = vec!["1".to_string()];
        let refreshed = vec![("1".to_string(), "Done".to_string())];
        let active = vec!["Todo".to_string(), "In Progress".to_string()];
        let terminal = vec!["Done".to_string(), "Closed".to_string()];
        let identifiers = vec![("1".to_string(), "TEST-1".to_string())];

        let actions = determine_reconcile_actions(&running, &refreshed, &active, &terminal, &identifiers);
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], ReconcileAction::TerminateAndClean { issue_id, .. } if issue_id == "1"));
    }

    #[test]
    fn test_determine_reconcile_actions_active() {
        let running = vec!["1".to_string()];
        let refreshed = vec![("1".to_string(), "In Progress".to_string())];
        let active = vec!["Todo".to_string(), "In Progress".to_string()];
        let terminal = vec!["Done".to_string(), "Closed".to_string()];
        let identifiers = vec![("1".to_string(), "TEST-1".to_string())];

        let actions = determine_reconcile_actions(&running, &refreshed, &active, &terminal, &identifiers);
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], ReconcileAction::UpdateSnapshot { issue_id } if issue_id == "1"));
    }

    #[test]
    fn test_determine_reconcile_actions_other_state() {
        let running = vec!["1".to_string()];
        let refreshed = vec![("1".to_string(), "Backlog".to_string())];
        let active = vec!["Todo".to_string(), "In Progress".to_string()];
        let terminal = vec!["Done".to_string(), "Closed".to_string()];
        let identifiers = vec![("1".to_string(), "TEST-1".to_string())];

        let actions = determine_reconcile_actions(&running, &refreshed, &active, &terminal, &identifiers);
        assert_eq!(actions.len(), 1);
        assert!(matches!(&actions[0], ReconcileAction::TerminateNoClean { issue_id, .. } if issue_id == "1"));
    }

    #[test]
    fn test_reconcile_stalled_runs_disabled() {
        let mut state = OrchestratorState::new(30_000, 10);
        // stall_timeout_ms <= 0 means disabled
        let result = reconcile_stalled_runs(&mut state, 0);
        assert!(result.is_empty());
        let result = reconcile_stalled_runs(&mut state, -1);
        assert!(result.is_empty());
    }
}
