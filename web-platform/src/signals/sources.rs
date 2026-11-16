//! Source payload parsers (§3 S2). Each returns redacted signals.

use serde_json::{json, Value};

use super::sanitize::{clean_details, clean_text, MAX_TEXT_CHARS, MAX_TITLE_CHARS};
use super::{IncomingSignal, Severity};

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Invalid(String),
    Unsupported(&'static str),
}

/// Parse a payload of `source`.
pub fn parse(source: &str, payload: &Value) -> Result<Vec<IncomingSignal>, ParseError> {
    match source {
        "generic" => generic(payload).map(|s| vec![s]),
        "sentry" => Err(ParseError::Unsupported("sentry")),
        "alertmanager" => Err(ParseError::Unsupported("alertmanager")),
        _ => Err(ParseError::Invalid(format!("unknown source {source}"))),
    }
}

fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)
        .and_then(|x| x.as_str())
        .filter(|s| !s.trim().is_empty())
}

/// `{title, fingerprint, severity?, message?, url?, details?, resolved?}`.
fn generic(v: &Value) -> Result<IncomingSignal, ParseError> {
    let title = str_field(v, "title").ok_or_else(|| ParseError::Invalid("title 必填".into()))?;
    let fingerprint = str_field(v, "fingerprint")
        .ok_or_else(|| ParseError::Invalid("fingerprint 必填".into()))?;
    if fingerprint.len() > 200 {
        return Err(ParseError::Invalid("fingerprint 最长 200".into()));
    }
    let url = str_field(v, "url")
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        .map(|u| clean_text(u, 1000));
    Ok(IncomingSignal {
        fingerprint: fingerprint.to_string(),
        title: clean_text(title, MAX_TITLE_CHARS),
        severity: str_field(v, "severity").and_then(Severity::parse),
        resolved: v.get("resolved").and_then(|r| r.as_bool()).unwrap_or(false),
        evidence: json!({
            "message": str_field(v, "message").map(|m| clean_text(m, MAX_TEXT_CHARS)),
            "details": v.get("details").map(clean_details),
        }),
        external_url: url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_requires_title_and_fingerprint() {
        assert!(matches!(
            parse("generic", &json!({"title": "t"})),
            Err(ParseError::Invalid(_))
        ));
        assert!(matches!(
            parse("generic", &json!({"fingerprint": "f"})),
            Err(ParseError::Invalid(_))
        ));
        let long = "f".repeat(201);
        assert!(matches!(
            parse("generic", &json!({"title": "t", "fingerprint": long})),
            Err(ParseError::Invalid(_))
        ));
        assert!(matches!(
            parse("nope", &json!({})),
            Err(ParseError::Invalid(_))
        ));
        assert_eq!(
            parse("sentry", &json!({})),
            Err(ParseError::Unsupported("sentry"))
        );
    }

    #[test]
    fn generic_maps_fields() {
        let s = parse(
            "generic",
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
            &json!({"title": "x", "fingerprint": "y", "severity": "loud", "url": "https://a.b/c"}),
        )
        .unwrap()
        .remove(0);
        assert_eq!(s.severity, None);
        assert_eq!(s.external_url.as_deref(), Some("https://a.b/c"));
    }
}
