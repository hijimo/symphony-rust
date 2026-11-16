//! Source payload parsers (§3 S2). Each returns redacted signals.

use serde_json::{json, Value};

use super::sanitize::{clean_details, clean_text, MAX_TEXT_CHARS, MAX_TITLE_CHARS};
use super::{IncomingSignal, Severity};

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Invalid(String),
}

/// Parse a payload of `source`. `resource` is Sentry's `Sentry-Hook-Resource`.
/// An empty list means "acknowledged, nothing to record".
pub fn parse(
    source: &str,
    resource: Option<&str>,
    payload: &Value,
) -> Result<Vec<IncomingSignal>, ParseError> {
    match source {
        "generic" => generic(payload).map(|s| vec![s]),
        "sentry" => sentry(resource, payload),
        "alertmanager" => alertmanager(payload),
        _ => Err(ParseError::Invalid(format!("unknown source {source}"))),
    }
}

fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)
        .and_then(|x| x.as_str())
        .filter(|s| !s.trim().is_empty())
}

fn http_url(u: Option<&str>) -> Option<String> {
    u.filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        .map(|u| clean_text(u, 1000))
}

/// `{title, fingerprint, severity?, message?, url?, details?, resolved?}`.
fn generic(v: &Value) -> Result<IncomingSignal, ParseError> {
    let title = str_field(v, "title").ok_or_else(|| ParseError::Invalid("title 必填".into()))?;
    let fingerprint = str_field(v, "fingerprint")
        .ok_or_else(|| ParseError::Invalid("fingerprint 必填".into()))?;
    if fingerprint.len() > 200 {
        return Err(ParseError::Invalid("fingerprint 最长 200".into()));
    }
    Ok(IncomingSignal {
        fingerprint: fingerprint.to_string(),
        title: clean_text(title, MAX_TITLE_CHARS),
        severity: str_field(v, "severity").and_then(Severity::parse),
        resolved: v.get("resolved").and_then(|r| r.as_bool()).unwrap_or(false),
        evidence: json!({
            "message": str_field(v, "message").map(|m| clean_text(m, MAX_TEXT_CHARS)),
            "details": v.get("details").map(clean_details),
        }),
        external_url: http_url(str_field(v, "url")),
    })
}

/// Sentry level → severity.
fn sentry_level(level: Option<&str>) -> Option<Severity> {
    match level? {
        "fatal" | "error" => Some(Severity::Critical),
        "warning" => Some(Severity::Warning),
        _ => Some(Severity::Info),
    }
}

/// Innermost stack frames first, at most 5, as `file:line in function`.
fn sentry_frames(event: &Value) -> Vec<String> {
    let Some(frames) = event
        .pointer("/exception/values/0/stacktrace/frames")
        .and_then(|f| f.as_array())
    else {
        return Vec::new();
    };
    frames
        .iter()
        .rev()
        .take(5)
        .map(|f| {
            let file = str_field(f, "filename")
                .or_else(|| str_field(f, "abs_path"))
                .unwrap_or("?");
            let line = f.get("lineno").and_then(|l| l.as_i64());
            let mut out = match line {
                Some(l) => format!("{file}:{l}"),
                None => file.to_string(),
            };
            if let Some(func) = str_field(f, "function") {
                out.push_str(&format!(" in {func}"));
            }
            clean_text(&out, 300)
        })
        .collect()
}

/// `issue` (created / unresolved / resolved) and `event_alert` webhooks.
/// Both use the Sentry issue id as fingerprint, so an alert and its issue
/// are one signal. Other resources and actions are acknowledged and ignored.
fn sentry(resource: Option<&str>, v: &Value) -> Result<Vec<IncomingSignal>, ParseError> {
    let action = str_field(v, "action").unwrap_or("");
    let missing = || ParseError::Invalid("Sentry 请求缺少 issue id".into());
    match (resource, action) {
        (Some("issue"), "created" | "unresolved" | "resolved") => {
            let i = v.pointer("/data/issue").ok_or_else(missing)?;
            let id = str_field(i, "id").ok_or_else(missing)?;
            let title = str_field(i, "title").unwrap_or("Sentry issue");
            Ok(vec![IncomingSignal {
                fingerprint: id.to_string(),
                title: clean_text(title, MAX_TITLE_CHARS),
                severity: sentry_level(str_field(i, "level")),
                resolved: action == "resolved",
                evidence: json!({
                    "message": str_field(i, "culprit").map(|m| clean_text(m, MAX_TEXT_CHARS)),
                    "details": clean_details(&json!({
                        "shortId": i.get("shortId"),
                        "project": i.pointer("/project/slug"),
                        "platform": i.get("platform"),
                        "level": i.get("level"),
                        "count": i.get("count"),
                        "userCount": i.get("userCount"),
                        "firstSeen": i.get("firstSeen"),
                        "lastSeen": i.get("lastSeen"),
                    })),
                }),
                external_url: http_url(
                    str_field(i, "web_url").or_else(|| str_field(i, "permalink")),
                ),
            }])
        }
        (Some("event_alert"), _) => {
            let e = v.pointer("/data/event").ok_or_else(missing)?;
            let id = str_field(e, "issue_id").ok_or_else(missing)?;
            let exception = match (
                e.pointer("/exception/values/0/type")
                    .and_then(|t| t.as_str()),
                e.pointer("/exception/values/0/value")
                    .and_then(|t| t.as_str()),
            ) {
                (Some(t), Some(val)) => Some(format!("{t}: {val}")),
                (Some(t), None) => Some(t.to_string()),
                _ => None,
            };
            let title = str_field(e, "title")
                .map(str::to_string)
                .or_else(|| exception.clone())
                .unwrap_or_else(|| "Sentry alert".to_string());
            Ok(vec![IncomingSignal {
                fingerprint: id.to_string(),
                title: clean_text(&title, MAX_TITLE_CHARS),
                severity: sentry_level(str_field(e, "level")),
                resolved: false,
                evidence: json!({
                    "message": str_field(e, "culprit").map(|m| clean_text(m, MAX_TEXT_CHARS)),
                    "details": clean_details(&json!({
                        "rule": v.pointer("/data/triggered_rule"),
                        "exception": exception,
                        "frames": sentry_frames(e),
                        "platform": e.get("platform"),
                        "datetime": e.get("datetime"),
                    })),
                }),
                external_url: http_url(str_field(e, "web_url")),
            }])
        }
        _ => Ok(Vec::new()),
    }
}

/// Alertmanager webhook (version 4): one signal per alert.
fn alertmanager(v: &Value) -> Result<Vec<IncomingSignal>, ParseError> {
    let alerts = v
        .get("alerts")
        .and_then(|a| a.as_array())
        .ok_or_else(|| ParseError::Invalid("Alertmanager 请求缺少 alerts".into()))?;
    alerts
        .iter()
        .map(|a| {
            let labels = a.get("labels").cloned().unwrap_or_else(|| json!({}));
            let name = str_field(&labels, "alertname")
                .ok_or_else(|| ParseError::Invalid("告警缺少 alertname".into()))?;
            let fingerprint = match str_field(a, "fingerprint") {
                Some(f) => f.to_string(),
                None => {
                    // Stable over label order: serde_json maps are sorted.
                    let sorted: std::collections::BTreeMap<String, Value> =
                        serde_json::from_value(labels.clone()).unwrap_or_default();
                    let text = serde_json::to_string(&sorted).unwrap_or_default();
                    use sha2::Digest;
                    format!(
                        "labels:{}",
                        &hex::encode(sha2::Sha256::digest(text.as_bytes()))[..16]
                    )
                }
            };
            let severity =
                str_field(&labels, "severity").map(|s| match s.to_ascii_lowercase().as_str() {
                    "critical" | "error" | "page" | "high" => Severity::Critical,
                    "info" | "none" | "low" => Severity::Info,
                    _ => Severity::Warning,
                });
            let title = a
                .pointer("/annotations/summary")
                .and_then(|s| s.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(name);
            Ok(IncomingSignal {
                fingerprint,
                title: clean_text(title, MAX_TITLE_CHARS),
                severity,
                resolved: str_field(a, "status") == Some("resolved"),
                evidence: json!({
                    "message": a
                        .pointer("/annotations/description")
                        .and_then(|s| s.as_str())
                        .map(|m| clean_text(m, MAX_TEXT_CHARS)),
                    "details": clean_details(&json!({
                        "labels": labels,
                        "startsAt": a.get("startsAt"),
                    })),
                }),
                external_url: http_url(str_field(a, "generatorURL")),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_requires_title_and_fingerprint() {
        assert!(matches!(
            parse("generic", None, &json!({"title": "t"})),
            Err(ParseError::Invalid(_))
        ));
        assert!(matches!(
            parse("generic", None, &json!({"fingerprint": "f"})),
            Err(ParseError::Invalid(_))
        ));
        let long = "f".repeat(201);
        assert!(matches!(
            parse("generic", None, &json!({"title": "t", "fingerprint": long})),
            Err(ParseError::Invalid(_))
        ));
        assert!(matches!(
            parse("nope", None, &json!({})),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn generic_maps_fields() {
        let s = parse(
            "generic",
            None,
            &json!({"title": "Disk", "fingerprint": "d1", "severity": "CRITICAL", "resolved": true,
                    "url": "javascript:alert(1)", "message": "m", "details": {"a": 1}}),
        )
        .unwrap()
        .remove(0);
        assert_eq!(s.severity, Some(Severity::Critical));
        assert!(s.resolved);
        assert_eq!(s.external_url, None, "only http(s) links");
        assert_eq!(s.evidence, json!({"message": "m", "details": {"a": 1}}));
        let s = parse(
            "generic",
            None,
            &json!({"title": "x", "fingerprint": "y", "severity": "loud", "url": "https://a.b/c"}),
        )
        .unwrap()
        .remove(0);
        assert_eq!(s.severity, None);
        assert_eq!(s.external_url.as_deref(), Some("https://a.b/c"));
    }

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/tests/fixtures/signals/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn sentry_issue_webhook() {
        let mut v = fixture("sentry_issue_created.json");
        let s = parse("sentry", Some("issue"), &v).unwrap().remove(0);
        assert_eq!(s.fingerprint, "1234567890");
        assert_eq!(s.title, "ZeroDivisionError: division by zero");
        assert_eq!(s.severity, Some(Severity::Critical), "fatal");
        assert!(!s.resolved);
        assert_eq!(
            s.external_url.as_deref(),
            Some("https://example-org.sentry.io/issues/1234567890/")
        );
        assert_eq!(s.evidence["message"], "app.billing in compute_invoice");
        assert_eq!(s.evidence["details"]["shortId"], "PYTHON-Y");
        assert_eq!(s.evidence["details"]["project"], "python");
        assert_eq!(s.evidence["details"]["count"], "3");

        v["action"] = json!("resolved");
        assert!(parse("sentry", Some("issue"), &v).unwrap()[0].resolved);
        v["action"] = json!("assigned");
        assert!(
            parse("sentry", Some("issue"), &v).unwrap().is_empty(),
            "no change to record"
        );
        v["action"] = json!("unresolved");
        v["data"]["issue"]["level"] = json!("warning");
        assert_eq!(
            parse("sentry", Some("issue"), &v).unwrap()[0].severity,
            Some(Severity::Warning)
        );
    }

    #[test]
    fn sentry_issue_alert_shares_the_issue_fingerprint() {
        let v = fixture("sentry_event_alert.json");
        let s = parse("sentry", Some("event_alert"), &v).unwrap().remove(0);
        assert_eq!(s.fingerprint, "1117540176");
        assert_eq!(s.title, "ReferenceError: heck is not defined");
        assert_eq!(s.severity, Some(Severity::Critical), "error");
        assert_eq!(s.evidence["details"]["rule"], "Very Important Alert Rule!");
        assert_eq!(
            s.evidence["details"]["exception"],
            "ReferenceError: heck is not defined"
        );
        let frames = s.evidence["details"]["frames"].as_array().unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0], "<anonymous>:3", "innermost frame first");
        assert_eq!(frames[1], "/js/prod/runner-4.1.7.min.js:1 in run");
        assert!(s
            .external_url
            .unwrap()
            .contains("/issues/1117540176/events/"));
    }

    #[test]
    fn sentry_other_resources_are_ignored_and_missing_ids_rejected() {
        assert!(parse(
            "sentry",
            Some("installation"),
            &json!({"action": "created"})
        )
        .unwrap()
        .is_empty());
        assert!(parse("sentry", None, &json!({})).unwrap().is_empty());
        assert!(matches!(
            parse(
                "sentry",
                Some("issue"),
                &json!({"action": "created", "data": {"issue": {"title": "x"}}})
            ),
            Err(ParseError::Invalid(_))
        ));
    }

    #[test]
    fn alertmanager_batch() {
        let v = fixture("alertmanager_v4.json");
        let list = parse("alertmanager", None, &v).unwrap();
        assert_eq!(list.len(), 2);
        let a = &list[0];
        assert_eq!(a.fingerprint, "c0ffee1234567890");
        assert_eq!(a.title, "Checkout 5xx rate above 5%");
        assert_eq!(a.severity, Some(Severity::Critical));
        assert!(!a.resolved);
        assert_eq!(a.evidence["message"], "5xx ratio is 7.2% over 10m");
        assert_eq!(a.evidence["details"]["labels"]["service"], "checkout");
        assert_eq!(a.evidence["details"]["startsAt"], "2026-11-14T08:00:00Z");
        assert_eq!(
            a.external_url.as_deref(),
            Some("http://prometheus.local/graph?g0.expr=rate")
        );
        let b = &list[1];
        assert_eq!(b.title, "DiskFull", "alertname when there is no summary");
        assert_eq!(b.severity, None, "default applies on insert");
        assert!(b.resolved);
    }

    #[test]
    fn alertmanager_without_fingerprint_hashes_labels() {
        let v = json!({"alerts": [
            {"status": "firing", "labels": {"b": "2", "alertname": "X", "a": "1"}},
            {"status": "firing", "labels": {"alertname": "X", "a": "1", "b": "2"}}
        ]});
        let list = parse("alertmanager", None, &v).unwrap();
        assert_eq!(
            list[0].fingerprint, list[1].fingerprint,
            "label order does not matter"
        );
        assert!(list[0].fingerprint.starts_with("labels:"));
        assert!(matches!(
            parse("alertmanager", None, &json!({"status": "firing"})),
            Err(ParseError::Invalid(_))
        ));
        assert!(
            matches!(
                parse("alertmanager", None, &json!({"alerts": [{"labels": {}}]})),
                Err(ParseError::Invalid(_))
            ),
            "an alert needs a name"
        );
    }
}
