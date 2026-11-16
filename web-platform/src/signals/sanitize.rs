//! Redaction of untrusted signal content before it is stored (§7).

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

pub const REDACTED: &str = "[已脱敏]";

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_TEXT_CHARS: usize = 4000;
pub const MAX_DETAILS_BYTES: usize = 8 * 1024;

fn patterns() -> &'static [(Regex, &'static str)] {
    static P: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    P.get_or_init(|| {
        vec![
            (
                Regex::new(r"(?s)-----BEGIN [A-Z0-9 ]+-----.*?(?:-----END [A-Z0-9 ]+-----|\z)")
                    .unwrap(),
                REDACTED,
            ),
            (
                Regex::new(r"(?:glpat-|ghp_|github_pat_|gho_|xoxb-|AKIA)[A-Za-z0-9_\-]*").unwrap(),
                REDACTED,
            ),
            (
                Regex::new(r#""(refresh_token|access_token|id_token|OPENAI_API_KEY)"\s*:\s*"[^"]*""#)
                    .unwrap(),
                // Without the quotes: the quoted key is itself a secret marker.
                "$1=[已脱敏]",
            ),
            (
                Regex::new(r#"(?i)\b(password|passwd|secret|api[_-]?key|token|authorization)(\s*[:=]\s*)[^\s,;"']+"#)
                    .unwrap(),
                "$1$2[已脱敏]",
            ),
        ]
    })
}

fn html_tags() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // Known HTML tags only: stack traces contain `<anonymous>`, `<module>`.
    R.get_or_init(|| {
        Regex::new(
            r"(?i)</?(?:a|b|i|u|p|br|hr|em|strong|span|div|font|img|iframe|script|style|code|pre|ul|ol|li|table|tr|td|th|h[1-6]|svg|object|embed|form|input|button|link|meta)\b[^>]*>",
        )
        .unwrap()
    })
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push('…');
    out
}

/// Remove credentials and HTML tags, then cap the length.
pub fn clean_text(s: &str, max_chars: usize) -> String {
    let mut out = html_tags().replace_all(s, "").into_owned();
    for (re, rep) in patterns() {
        out = re.replace_all(&out, *rep).into_owned();
    }
    // Anything the markers of the document scanner still see is dropped.
    if crate::sdlc::content::find_secret(&out, &[]).is_some() {
        return REDACTED.to_string();
    }
    truncate(out.trim(), max_chars)
}

/// Clean every string inside `v`; replace the value by a truncated string
/// when its JSON form is larger than `MAX_DETAILS_BYTES`.
pub fn clean_details(v: &Value) -> Value {
    fn walk(v: &Value) -> Value {
        match v {
            Value::String(s) => Value::String(clean_text(s, MAX_TEXT_CHARS)),
            Value::Array(a) => Value::Array(a.iter().map(walk).collect()),
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, v)| (clean_text(k, MAX_TITLE_CHARS), walk(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    let cleaned = walk(v);
    let text = cleaned.to_string();
    if text.len() <= MAX_DETAILS_BYTES {
        cleaned
    } else {
        let mut cut = MAX_DETAILS_BYTES;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        Value::String(format!("{}…", &text[..cut]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn credentials_are_redacted() {
        let cases = [
            ("token glpat-abc123DEF_x leaked", "token [已脱敏] leaked"),
            ("key AKIAIOSFODNN7EXAMPLE end", "key [已脱敏] end"),
            ("password=hunter2, user=bob", "password=[已脱敏], user=bob"),
            ("Authorization: Bearer", "Authorization: [已脱敏]"),
            (r#"{"access_token": "abc"}"#, "{access_token=[已脱敏]}"),
            (
                "a -----BEGIN PRIVATE KEY-----\nxyz\n-----END PRIVATE KEY----- b",
                "a [已脱敏] b",
            ),
            ("a -----BEGIN PRIVATE KEY-----\nunterminated", "a [已脱敏]"),
        ];
        for (input, want) in cases {
            assert_eq!(clean_text(input, 1000), want, "{input}");
        }
    }

    #[test]
    fn html_is_stripped_and_text_truncated() {
        assert_eq!(clean_text("<b>Disk</b> full", 100), "Disk full");
        assert_eq!(clean_text("abcdef", 3), "abc…");
        assert_eq!(clean_text("  x  ", 10), "x");
        assert_eq!(
            clean_text("at <anonymous>:3 in <module>", 100),
            "at <anonymous>:3 in <module>"
        );
        assert_eq!(clean_text("<IMG src=x onerror=1>a<br/>b", 100), "ab");
    }

    #[test]
    fn details_are_cleaned_recursively_and_capped() {
        let v = clean_details(&json!({"env": {"t": "ghp_secret123"}, "n": 3, "l": ["<i>a</i>"]}));
        assert_eq!(v, json!({"env": {"t": "[已脱敏]"}, "n": 3, "l": ["a"]}));
        let big = clean_details(
            &json!({"s": "y".repeat(3000), "t": "z".repeat(3000), "u": "w".repeat(3000)}),
        );
        let s = big.as_str().expect("oversized details become a string");
        assert!(s.len() <= MAX_DETAILS_BYTES + 3 && s.ends_with('…'));
    }
}
