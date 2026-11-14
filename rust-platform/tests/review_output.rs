//! Parsing Codex `review/start` output (P2 design §4 B1, risk R1).
//!
//! The fixture was recorded from codex-cli 0.158.0 (`review/start` with a
//! `baseBranch` target); workspace paths were anonymized.

use symphony_platform::agent::review_output::{
    extract_review_text, parse_review, ReviewFinding, Severity,
};

const FIXTURE: &str = include_str!("fixtures/codex_review/review_start_0.158.json");
const WS: &str = "/srv/symphony/ws/i-1-_7";

fn fixture_messages() -> Vec<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    v["messages"].as_array().unwrap().clone()
}

#[test]
fn test_extract_review_text_takes_exited_review_mode_item() {
    let text = extract_review_text(&fixture_messages()).expect("exitedReviewMode");
    assert!(text.contains("Full review comments:"));
    // enteredReviewMode carries only the target description.
    assert!(!text.starts_with("changes against"));
}

#[test]
fn test_parse_recorded_codex_review() {
    let text = extract_review_text(&fixture_messages()).unwrap();
    let review = parse_review(&text, Some(WS));
    assert!(review.parsed);
    assert!(review.summary.starts_with("补丁把 `average`"));
    assert_eq!(review.findings.len(), 2);
    assert_eq!(review.important(), 1);
    assert_eq!(review.nit(), 1);

    let first = &review.findings[0];
    assert_eq!(first.priority, 1);
    assert_eq!(first.severity(), Severity::Important);
    assert!(first.title.starts_with("average() 用 len(xs)-1"));
    assert_eq!(first.location.as_deref(), Some("calc.py:1-3"));
    assert!(first.body.contains("sum(xs) / len(xs)"));
    assert!(!first.body.starts_with(' '));

    let second = &review.findings[1];
    assert_eq!(second.priority, 2);
    assert_eq!(second.severity(), Severity::Nit);
    assert_eq!(second.location.as_deref(), Some("calc.py:5-6"));
}

#[test]
fn test_parse_review_without_findings_is_parsed_with_zero_counts() {
    let review = parse_review("The change looks correct; no issues found.", None);
    assert!(review.parsed);
    assert!(review.findings.is_empty());
    assert_eq!(review.important(), 0);
}

#[test]
fn test_parse_review_p0_and_p3_and_location_outside_workspace() {
    let text = "Summary.\n\nFull review comments:\n\n- [P0] Data loss — /etc/x.rs:3\n  body a\n  continued\n\n- [P3] Typo in docs\n  fix spelling\n";
    let review = parse_review(text, Some(WS));
    assert_eq!(
        review.findings,
        vec![
            ReviewFinding {
                priority: 0,
                title: "Data loss".into(),
                location: Some("/etc/x.rs:3".into()),
                body: "body a\ncontinued".into(),
            },
            ReviewFinding {
                priority: 3,
                title: "Typo in docs".into(),
                location: None,
                body: "fix spelling".into(),
            },
        ]
    );
    assert_eq!(review.important(), 1);
    assert_eq!(review.nit(), 1);
}

#[test]
fn test_unparseable_markers_make_counts_unknown() {
    // A priority tag that is not a list item means the format changed.
    let review = parse_review("Found [P1] something but in prose", None);
    assert!(!review.parsed);
    // Empty output is not a valid review either.
    assert!(!parse_review("   ", None).parsed);
    // Priorities outside P0–P3 are not accepted.
    assert!(!parse_review("Full review comments:\n\n- [P7] odd\n  x\n", None).parsed);
}

#[test]
fn test_extract_review_text_none_when_review_never_finished() {
    let msgs: Vec<serde_json::Value> = fixture_messages()
        .into_iter()
        .filter(|m| m["params"]["item"]["type"].as_str() != Some("exitedReviewMode"))
        .collect();
    assert!(extract_review_text(&msgs).is_none());
}
