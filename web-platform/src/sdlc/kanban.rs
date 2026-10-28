//! Kanban stage columns (flow design §9): a pure label → (column, sub-state)
//! mapping. Display only: labels are never written here.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum StageColumn {
    Plan,
    Design,
    Build,
    Deploy,
    Maintain,
}

impl StageColumn {
    pub const ALL: [StageColumn; 5] = [
        StageColumn::Plan,
        StageColumn::Design,
        StageColumn::Build,
        StageColumn::Deploy,
        StageColumn::Maintain,
    ];

    pub fn title(self) -> &'static str {
        match self {
            StageColumn::Plan => "计划",
            StageColumn::Design => "设计",
            StageColumn::Build => "构建与测试",
            StageColumn::Deploy => "部署",
            StageColumn::Maintain => "运维",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            StageColumn::Plan => "intent",
            StageColumn::Design => "spec",
            StageColumn::Build => "plan → 实现 → 测试",
            StageColumn::Deploy => "代码审查与合并",
            StageColumn::Maintain => "近期完成",
        }
    }
}

/// Ordered mapping; later stages win when several labels are present
/// (§9.4 "越靠后的阶段越优先").
const MAP: &[(&str, StageColumn, &str)] = &[
    ("Backlog", StageColumn::Plan, "待启动"),
    ("doc:intent", StageColumn::Plan, "起草 intent"),
    ("doc:intent-review", StageColumn::Plan, "待审 intent"),
    ("doc:spec", StageColumn::Design, "AI 起草 spec"),
    ("doc:spec-review", StageColumn::Design, "待审 spec"),
    ("doc:plan", StageColumn::Build, "AI 起草 plan"),
    ("doc:plan-review", StageColumn::Build, "待审 plan"),
    ("Todo", StageColumn::Build, "排队"),
    ("symphony-claimed", StageColumn::Build, "实现中"),
    ("In Progress", StageColumn::Build, "实现中"),
    ("Rework", StageColumn::Build, "返工"),
    ("Testing", StageColumn::Build, "测试中"),
    ("Human Review", StageColumn::Deploy, "待审 MR"),
    ("Merging", StageColumn::Deploy, "合并中"),
];

/// Labels that mark a status (used to detect label anomalies).
pub fn flow_labels() -> impl Iterator<Item = &'static str> {
    MAP.iter().map(|(l, _, _)| *l)
}

/// Labels the board must fetch explicitly (open issues).
pub fn query_labels() -> Vec<&'static str> {
    MAP.iter()
        .map(|(l, _, _)| *l)
        .filter(|l| *l != "Backlog")
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub column: StageColumn,
    pub sub_state: String,
    /// More than one status label on the issue.
    pub anomaly: bool,
    /// Waiting on a human (review states).
    pub waiting_human: bool,
}

/// Place an open issue. Unlabeled issues are "待启动" in 计划.
pub fn place_open(labels: &[String]) -> Placement {
    let hits: Vec<&(&str, StageColumn, &str)> = MAP
        .iter()
        .filter(|(l, _, _)| labels.iter().any(|x| x == l))
        .collect();
    let anomaly = hits.iter().filter(|(l, _, _)| *l != "Backlog").count() > 1;
    match hits.iter().max_by_key(|(_, c, _)| *c) {
        // Within the winning column take the last (most advanced) mapping.
        Some((_, col, _)) => {
            let (label, _, sub) = hits.iter().rev().find(|(_, c, _)| c == col).unwrap();
            Placement {
                column: *col,
                sub_state: sub.to_string(),
                anomaly,
                waiting_human: label.ends_with("-review") || *label == "Human Review",
            }
        }
        None => Placement {
            column: StageColumn::Plan,
            sub_state: "待启动".to_string(),
            anomaly: false,
            waiting_human: false,
        },
    }
}

/// Closed issues: "运维" unless rejected in the document flow.
pub fn place_closed() -> Placement {
    Placement {
        column: StageColumn::Maintain,
        sub_state: "已上线".to_string(),
        anomaly: false,
        waiting_human: false,
    }
}

/// `track:*` label shown on the card.
pub fn track_of(labels: &[String]) -> Option<String> {
    labels
        .iter()
        .find(|l| l.starts_with("track:"))
        .map(|l| l.trim_start_matches("track:").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn mapping() {
        assert_eq!(place_open(&l(&[])).column, StageColumn::Plan);
        assert_eq!(place_open(&l(&["Backlog"])).sub_state, "待启动");
        assert_eq!(
            place_open(&l(&["doc:intent-review"])).column,
            StageColumn::Plan
        );
        assert!(place_open(&l(&["doc:intent-review"])).waiting_human);
        assert_eq!(place_open(&l(&["doc:spec"])).column, StageColumn::Design);
        assert_eq!(
            place_open(&l(&["doc:plan-review"])).column,
            StageColumn::Build
        );
        assert_eq!(place_open(&l(&["Todo", "track:fast"])).sub_state, "排队");
        assert_eq!(place_open(&l(&["Testing"])).sub_state, "测试中");
        assert_eq!(
            place_open(&l(&["Human Review"])).column,
            StageColumn::Deploy
        );
        assert!(place_open(&l(&["Human Review"])).waiting_human);
        assert_eq!(place_closed().column, StageColumn::Maintain);
    }

    #[test]
    fn later_stage_wins_and_flags_anomaly() {
        let p = place_open(&l(&["doc:spec", "In Progress"]));
        assert_eq!(p.column, StageColumn::Build);
        assert!(p.anomaly);
        let p = place_open(&l(&["Backlog", "Todo"]));
        assert!(!p.anomaly);
        assert_eq!(p.column, StageColumn::Build);
    }

    #[test]
    fn track_label() {
        assert_eq!(track_of(&l(&["track:full", "x"])), Some("full".into()));
        assert_eq!(track_of(&l(&["x"])), None);
        assert!(!query_labels().contains(&"Backlog"));
    }
}
