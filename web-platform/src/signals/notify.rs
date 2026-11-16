//! Notify the project's channels about new and recurring signals
//! (docs/方案/ai-native-sdlc-p3-design.md §3 S5).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use dashmap::DashMap;

use super::store::Signal;
use crate::notification::{AlertEvent, NotificationDispatcher, Severity};

/// At most one notification per signal in this window.
pub const NOTIFY_WINDOW: Duration = Duration::from_secs(30 * 60);

/// Rule id of signal notifications (channel severity filters still apply).
pub const RULE_ID: &str = "external_signal";

/// Why a signal is notified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    New,
    Recurred,
}

/// The notification for a signal; `None` for `info` signals.
pub fn signal_event(project_name: &str, s: &Signal, reason: Reason) -> Option<AlertEvent> {
    let severity = match s.severity.as_str() {
        "critical" => Severity::Critical,
        "warning" => Severity::Warning,
        _ => return None,
    };
    let what = match reason {
        Reason::New => "新信号待分诊",
        Reason::Recurred => "信号复发，待分诊",
    };
    let mut message = format!(
        "{what}：{}（来源 {}，触发 {} 次）。在项目看板“运维”列处理：/projects/{}/kanban",
        s.title, s.source, s.count, s.project_id
    );
    if let (Reason::Recurred, Some(iid)) = (reason, s.issue_iid) {
        message.push_str(&format!("。原 Issue #{iid} 已关闭"));
    }
    let mut context = HashMap::new();
    context.insert("signal_id".to_string(), s.id.to_string());
    context.insert("source".to_string(), s.source.clone());
    if let Some(u) = &s.external_url {
        context.insert("url".to_string(), u.clone());
    }
    Some(AlertEvent {
        id: uuid::Uuid::new_v4().to_string(),
        rule_id: RULE_ID.to_string(),
        severity,
        project_id: Some(s.project_id),
        project_name: Some(project_name.to_string()),
        title: format!("[信号] {}", s.title),
        message,
        context,
        fired_at: chrono::Utc::now(),
    })
}

/// `true` when the signal was not notified within `NOTIFY_WINDOW`; records `now`.
pub fn should_notify(last: &DashMap<i64, Instant>, signal_id: i64, now: Instant) -> bool {
    if let Some(t) = last.get(&signal_id) {
        if now.duration_since(*t) < NOTIFY_WINDOW {
            return false;
        }
    }
    last.insert(signal_id, now);
    true
}

/// Send the signal notification. `last` remembers when each signal was
/// notified. Returns whether a notification was dispatched.
pub async fn notify(
    dispatcher: &NotificationDispatcher,
    last: &DashMap<i64, Instant>,
    project_name: &str,
    s: &Signal,
    reason: Reason,
) -> bool {
    let Some(event) = signal_event(project_name, s, reason) else {
        return false;
    };
    if !should_notify(last, s.id, Instant::now()) {
        return false;
    }
    dispatcher.dispatch(&event).await;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::{NotificationChannel, NotificationError, NotificationResult};
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn sig(id: i64, severity: &str) -> Signal {
        Signal {
            id,
            project_id: 3,
            source: "sentry".into(),
            fingerprint: "f".into(),
            title: "Checkout 500".into(),
            severity: severity.into(),
            status: "new".into(),
            resolved: false,
            first_seen_at: "t".into(),
            last_seen_at: "t".into(),
            count: 4,
            evidence: json!({}),
            external_url: Some("https://sentry.local/1".into()),
            flow_id: None,
            issue_iid: Some(12),
            ignored_until: None,
            triaged_by: None,
            triaged_at: None,
        }
    }

    pub(crate) struct Recorder(pub Mutex<Vec<AlertEvent>>);

    #[async_trait]
    impl NotificationChannel for Recorder {
        fn channel_type(&self) -> &str {
            "test"
        }
        fn channel_id(&self) -> &str {
            "rec"
        }
        async fn send(&self, alert: &AlertEvent) -> Result<NotificationResult, NotificationError> {
            self.0.lock().unwrap().push(alert.clone());
            Ok(NotificationResult {
                channel_id: "rec".into(),
                channel_type: "test".into(),
                success: true,
                error_message: None,
                response_time_ms: 0,
                sent_at: chrono::Utc::now(),
            })
        }
        async fn send_test(
            &self,
            _: &str,
            _: &str,
        ) -> Result<NotificationResult, NotificationError> {
            unreachable!()
        }
        async fn health_check(&self) -> bool {
            true
        }
    }

    #[test]
    fn events_map_severity_and_skip_info() {
        let e = signal_event("shop", &sig(1, "critical"), Reason::New).unwrap();
        assert_eq!(e.severity, Severity::Critical);
        assert_eq!(e.title, "[信号] Checkout 500");
        assert!(
            e.message.contains("新信号待分诊") && e.message.contains("/projects/3/kanban"),
            "{}",
            e.message
        );
        assert!(
            !e.message.contains("#12"),
            "new signals do not mention the old issue"
        );
        assert_eq!(e.project_name.as_deref(), Some("shop"));
        assert_eq!(e.context["url"], "https://sentry.local/1");
        let r = signal_event("shop", &sig(1, "warning"), Reason::Recurred).unwrap();
        assert_eq!(r.severity, Severity::Warning);
        assert!(
            r.message.contains("复发") && r.message.contains("原 Issue #12"),
            "{}",
            r.message
        );
        assert!(signal_event("shop", &sig(1, "info"), Reason::New).is_none());
    }

    #[test]
    fn notifications_are_throttled_per_signal() {
        let m = DashMap::new();
        let t0 = Instant::now();
        assert!(should_notify(&m, 1, t0));
        assert!(!should_notify(&m, 1, t0 + Duration::from_secs(60)));
        assert!(should_notify(&m, 2, t0), "other signals are independent");
        assert!(should_notify(
            &m,
            1,
            t0 + NOTIFY_WINDOW + Duration::from_secs(1)
        ));
    }

    #[tokio::test]
    async fn notify_dispatches_once_through_the_channel() {
        let rec = Arc::new(Recorder(Mutex::new(Vec::new())));
        let d = NotificationDispatcher::new();
        d.add_channel(rec.clone(), vec!["critical".into(), "warning".into()], true)
            .await;
        let last = DashMap::new();
        assert!(notify(&d, &last, "shop", &sig(1, "critical"), Reason::New).await);
        assert!(!notify(&d, &last, "shop", &sig(1, "critical"), Reason::Recurred).await);
        assert!(!notify(&d, &last, "shop", &sig(2, "info"), Reason::New).await);
        assert_eq!(rec.0.lock().unwrap().len(), 1);
    }
}
