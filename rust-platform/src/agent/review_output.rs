//! Parsing the output of Codex `review/start` (P2 design §4 B1).
//!
//! Format observed with codex-cli 0.158.0 (fixture
//! `tests/fixtures/codex_review/review_start_0.158.json`):
//!
//! ```text
//! <summary paragraph>
//!
//! Full review comments:
//!
//! - [P1] <title> — <abs path>:<lines>
//!   <body, indented>
//! ```
//!
//! The format is not documented, so anything unexpected yields
//! `parsed == false` and callers treat the counts as unknown.

/// Severity bucket used by Symphony (`REVIEW.md` may redefine meanings, the
/// mapping from Codex priorities stays fixed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// P0 / P1
    Important,
    /// P2 / P3
    Nit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewFinding {
    /// 0–3 from `[P0]`–`[P3]`.
    pub priority: u8,
    pub title: String,
    /// `path:lines`, relative to the workspace when it was inside it.
    pub location: Option<String>,
    pub body: String,
}

impl ReviewFinding {
    pub fn severity(&self) -> Severity {
        if self.priority <= 1 {
            Severity::Important
        } else {
            Severity::Nit
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedReview {
    pub summary: String,
    pub findings: Vec<ReviewFinding>,
    /// False when the output did not match the known format.
    pub parsed: bool,
}

impl ParsedReview {
    pub fn important(&self) -> usize {
        self.count(Severity::Important)
    }

    pub fn nit(&self) -> usize {
        self.count(Severity::Nit)
    }

    fn count(&self, s: Severity) -> usize {
        self.findings.iter().filter(|f| f.severity() == s).count()
    }
}

/// Review text from the `exitedReviewMode` item (last one wins).
pub fn extract_review_text(messages: &[serde_json::Value]) -> Option<String> {
    messages
        .iter()
        .rev()
        .filter(|m| m.get("method").and_then(|v| v.as_str()) == Some("item/completed"))
        .filter_map(|m| m.pointer("/params/item"))
        .filter(|item| item.get("type").and_then(|t| t.as_str()) == Some("exitedReviewMode"))
        .find_map(|item| item.get("review").and_then(|r| r.as_str()))
        .map(str::to_string)
}

const SECTION: &str = "Full review comments:";

/// Parse the review text. `workspace` makes absolute locations relative.
pub fn parse_review(text: &str, workspace: Option<&str>) -> ParsedReview {
    let text = text.trim();
    let (summary, comments) = match text.find(SECTION) {
        Some(i) => (text[..i].trim(), Some(&text[i + SECTION.len()..])),
        None => (text, None),
    };
    let mut findings = Vec::new();
    let mut ok = !text.is_empty();
    if let Some(comments) = comments {
        let mut current: Option<ReviewFinding> = None;
        for line in comments.lines() {
            if let Some(rest) = line.strip_prefix("- [P") {
                if let Some(f) = current.take() {
                    findings.push(f);
                }
                match parse_header(rest, workspace) {
                    Some(f) => current = Some(f),
                    None => ok = false,
                }
            } else if let Some(f) = current.as_mut() {
                let body = line.trim();
                if !body.is_empty() {
                    if !f.body.is_empty() {
                        f.body.push('\n');
                    }
                    f.body.push_str(body);
                }
            } else if !line.trim().is_empty() {
                ok = false;
            }
        }
        findings.extend(current);
    } else if summary.contains("[P") {
        // Priority tags outside the known section: the format changed.
        ok = false;
    }
    ParsedReview {
        summary: summary.to_string(),
        findings,
        parsed: ok,
    }
}

/// `1] title — /abs/path:1-3` (after the `- [P` prefix).
fn parse_header(rest: &str, workspace: Option<&str>) -> Option<ReviewFinding> {
    let (digit, rest) = rest.split_once(']')?;
    let priority: u8 = digit.parse().ok().filter(|p| *p <= 3)?;
    let rest = rest.trim();
    let (title, location) = match rest.rsplit_once(" — ") {
        Some((t, loc)) if loc.contains(':') => (t.trim(), Some(relative(loc.trim(), workspace))),
        _ => (rest, None),
    };
    if title.is_empty() {
        return None;
    }
    Some(ReviewFinding {
        priority,
        title: title.to_string(),
        location,
        body: String::new(),
    })
}

fn relative(location: &str, workspace: Option<&str>) -> String {
    workspace
        .map(|w| w.trim_end_matches('/'))
        .and_then(|w| location.strip_prefix(w))
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(location)
        .to_string()
}
