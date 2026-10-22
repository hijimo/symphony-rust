//! Cross-repository MR aggregation for multi-repo projects (design §7.1, §7.2).

use crate::models::PlatformMergeRequest;

/// The trailer that ties an MR in any repository to the primary issue.
pub fn issue_trailer(primary_slug: &str, iid: u64) -> String {
    format!("Symphony-Issue: {primary_slug}#{iid}")
}

/// True when `description` carries the trailer for this issue on its own line.
pub fn has_issue_trailer(description: Option<&str>, primary_slug: &str, iid: u64) -> bool {
    let want = issue_trailer(primary_slug, iid);
    description
        .unwrap_or_default()
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case(&want))
}

/// Keep MRs of the primary project as today; MRs from other projects only
/// count when they carry the trailer (filters out casual mentions).
pub fn filter_related(
    mrs: Vec<PlatformMergeRequest>,
    primary_slug: &str,
    iid: u64,
    multi_repo: bool,
) -> Vec<PlatformMergeRequest> {
    if !multi_repo {
        return mrs;
    }
    mrs.into_iter()
        .filter(|mr| {
            let own = mr.target_project_path.as_deref().unwrap_or(primary_slug);
            own.eq_ignore_ascii_case(primary_slug)
                || has_issue_trailer(mr.description.as_deref(), primary_slug, iid)
        })
        .collect()
}

/// MRs that are still open although their issue is closed (orphans, §7.2).
pub fn orphan_mrs<'a>(
    mrs: &'a [PlatformMergeRequest],
    primary_slug: &str,
    iid: u64,
) -> Vec<&'a PlatformMergeRequest> {
    mrs.iter()
        .filter(|mr| mr.state == "opened")
        .filter(|mr| has_issue_trailer(mr.description.as_deref(), primary_slug, iid))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PlatformUser;

    fn mr(iid: u64, path: &str, state: &str, desc: Option<&str>) -> PlatformMergeRequest {
        PlatformMergeRequest {
            iid,
            platform_node_id: None,
            title: format!("mr {iid}"),
            description: desc.map(str::to_string),
            state: state.to_string(),
            author: PlatformUser {
                username: "u".into(),
                display_name: None,
                avatar_url: None,
            },
            source_project_path: Some(path.to_string()),
            target_project_path: Some(path.to_string()),
            source_branch: "b".into(),
            target_branch: "main".into(),
            ci_status: None,
            ci_web_url: None,
            review_status: None,
            reviewers: vec![],
            merge_status: None,
            related_issue_iids: vec![],
            additions: None,
            deletions: None,
            changed_files: None,
            created_at: String::new(),
            updated_at: String::new(),
            merged_at: None,
            web_url: String::new(),
        }
    }

    const TRAILER: &str = "Fix\n\n<!-- symphony:multi-repo -->\nSymphony-Issue: zimei10525/jst-ai-studio#32\nRelated: x";

    #[test]
    fn trailer_must_match_issue_exactly() {
        assert!(has_issue_trailer(
            Some(TRAILER),
            "zimei10525/jst-ai-studio",
            32
        ));
        assert!(!has_issue_trailer(
            Some(TRAILER),
            "zimei10525/jst-ai-studio",
            3
        ));
        assert!(!has_issue_trailer(
            Some("see Symphony-Issue: zimei10525/jst-ai-studio#32 maybe"),
            "zimei10525/jst-ai-studio",
            32
        ));
        assert!(!has_issue_trailer(None, "a/b", 1));
    }

    #[test]
    fn filter_keeps_primary_and_trailer_mrs_only_in_multi_repo() {
        let mrs = vec![
            mr(87, "zimei10525/jst-ai-studio", "opened", Some("Closes #32")),
            mr(123, "JstAi/jxy", "opened", Some(TRAILER)),
            mr(
                9,
                "JstAi/jxy",
                "opened",
                Some("mentions zimei10525/jst-ai-studio#32"),
            ),
        ];
        let kept: Vec<u64> = filter_related(mrs.clone(), "zimei10525/jst-ai-studio", 32, true)
            .iter()
            .map(|m| m.iid)
            .collect();
        assert_eq!(kept, vec![87, 123]);
        assert_eq!(
            filter_related(mrs, "zimei10525/jst-ai-studio", 32, false).len(),
            3
        );
    }

    #[test]
    fn orphans_are_open_trailer_mrs() {
        let mrs = vec![
            mr(123, "JstAi/jxy", "opened", Some(TRAILER)),
            mr(124, "JstAi/jxy", "merged", Some(TRAILER)),
            mr(125, "JstAi/jxy", "opened", None),
        ];
        let o: Vec<u64> = orphan_mrs(&mrs, "zimei10525/jst-ai-studio", 32)
            .iter()
            .map(|m| m.iid)
            .collect();
        assert_eq!(o, vec![123]);
    }
}
