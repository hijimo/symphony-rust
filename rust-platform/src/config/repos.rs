//! Multi-repository workspace configuration (`workspace.repos`).
//!
//! Design: docs/方案/multi-repo-workspace-design.md §4.1. When the list is
//! empty the runtime keeps the legacy single-repository behavior untouched.

use std::collections::{HashMap, HashSet};

use serde_yaml::Value as YamlValue;

use super::service_config::ServiceConfigError;

/// Directory names under the workspace root that a repo alias must not shadow.
const RESERVED_ALIASES: &[&str] = &[".symphony", "symphony", "tmp", "quarantine"];

/// Role of a repository inside a multi-repo workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoRole {
    Primary,
    Secondary,
}

impl RepoRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            RepoRole::Primary => "primary",
            RepoRole::Secondary => "secondary",
        }
    }
}

/// Whether the agent may commit/push in the repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoMode {
    Read,
    Write,
}

/// Clone strategy. `Partial` = `--filter=blob:none` (never `--depth`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckoutMode {
    Full,
    Partial,
}

/// One entry of `workspace.repos`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSpec {
    pub alias: String,
    /// `namespace/repo_name` on the tracker's platform instance.
    pub slug: String,
    pub role: RepoRole,
    pub default_branch: String,
    pub mode: RepoMode,
    pub checkout: CheckoutMode,
    pub merge_after: Vec<String>,
}

impl RepoSpec {
    pub fn writable(&self) -> bool {
        self.mode == RepoMode::Write
    }

    /// `repo_name` part of the slug.
    pub fn repo_name(&self) -> &str {
        self.slug.rsplit('/').next().unwrap_or(&self.slug)
    }
}

/// Normalize a repository name into the branch key used by D4:
/// lowercase, any char outside `[a-z0-9-]` replaced by `-`.
pub fn primary_key(repo_name: &str) -> String {
    repo_name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Parse and validate the `repos` list of the `workspace` section.
///
/// `tracker_project_slug` must equal the primary entry's slug.
/// `after_create` is checked for a clone into the workspace root.
pub fn parse_repos(
    workspace: &HashMap<String, YamlValue>,
    tracker_project_slug: &str,
    after_create: Option<&str>,
) -> Result<Vec<RepoSpec>, ServiceConfigError> {
    let Some(raw) = workspace.get("repos") else {
        return Ok(Vec::new());
    };
    let seq = match raw {
        YamlValue::Null => return Ok(Vec::new()),
        YamlValue::Sequence(seq) => seq,
        _ => {
            return Err(invalid("workspace.repos", "must be a list"));
        }
    };
    if seq.is_empty() {
        return Ok(Vec::new());
    }

    let mut repos = Vec::with_capacity(seq.len());
    for (idx, item) in seq.iter().enumerate() {
        repos.push(parse_entry(idx, item)?);
    }
    validate_repos(&repos, tracker_project_slug)?;

    if let Some(script) = after_create {
        if clones_into_workspace_root(script) {
            return Err(invalid(
                "hooks.after_create",
                "must not clone into the workspace root when workspace.repos is set \
                 (repositories are checked out by Symphony into <workspace>/<alias>/)",
            ));
        }
    }

    Ok(repos)
}

fn parse_entry(idx: usize, item: &YamlValue) -> Result<RepoSpec, ServiceConfigError> {
    let field = |name: &str| format!("workspace.repos[{idx}].{name}");
    let YamlValue::Mapping(map) = item else {
        return Err(invalid(
            &format!("workspace.repos[{idx}]"),
            "must be a mapping",
        ));
    };
    let get_str = |key: &str| -> Option<String> {
        map.get(YamlValue::String(key.to_string()))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    let alias = get_str("alias").ok_or_else(|| invalid(&field("alias"), "is required"))?;
    let slug = get_str("slug").ok_or_else(|| invalid(&field("slug"), "is required"))?;
    let default_branch = get_str("default_branch")
        .ok_or_else(|| invalid(&field("default_branch"), "is required"))?;
    let role = match get_str("role").as_deref() {
        Some("primary") => RepoRole::Primary,
        Some("secondary") | None => RepoRole::Secondary,
        Some(other) => {
            return Err(invalid(
                &field("role"),
                &format!("unknown role '{other}' (primary | secondary)"),
            ))
        }
    };
    let mode = match get_str("mode").as_deref() {
        Some("write") => RepoMode::Write,
        Some("read") | None => RepoMode::Read,
        Some(other) => {
            return Err(invalid(
                &field("mode"),
                &format!("unknown mode '{other}' (read | write)"),
            ))
        }
    };
    let checkout = match get_str("checkout").as_deref() {
        Some("full") => CheckoutMode::Full,
        Some("partial") | None => CheckoutMode::Partial,
        Some(other) => {
            return Err(invalid(
                &field("checkout"),
                &format!("unknown checkout '{other}' (full | partial)"),
            ))
        }
    };
    let merge_after = match map.get(YamlValue::String("merge_after".to_string())) {
        None | Some(YamlValue::Null) => Vec::new(),
        Some(YamlValue::Sequence(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| invalid(&field("merge_after"), "must be a list of aliases"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(invalid(&field("merge_after"), "must be a list of aliases")),
    };

    Ok(RepoSpec {
        alias,
        slug,
        role,
        default_branch,
        mode,
        checkout,
        merge_after,
    })
}

fn validate_repos(
    repos: &[RepoSpec],
    tracker_project_slug: &str,
) -> Result<(), ServiceConfigError> {
    let mut seen = HashSet::new();
    for repo in repos {
        if !is_valid_alias(&repo.alias) {
            return Err(invalid(
                "workspace.repos.alias",
                &format!("'{}' must match [a-z0-9-]+", repo.alias),
            ));
        }
        if RESERVED_ALIASES.contains(&repo.alias.as_str()) {
            return Err(invalid(
                "workspace.repos.alias",
                &format!("'{}' is reserved", repo.alias),
            ));
        }
        if !seen.insert(repo.alias.clone()) {
            return Err(invalid(
                "workspace.repos.alias",
                &format!("duplicate alias '{}'", repo.alias),
            ));
        }
        if repo.slug.split('/').filter(|p| !p.is_empty()).count() < 2
            || repo.slug.contains("..")
            || repo.slug.contains("://")
        {
            return Err(invalid(
                "workspace.repos.slug",
                &format!("'{}' must be namespace/repo_name", repo.slug),
            ));
        }
        if !is_safe_ref(&repo.default_branch) {
            return Err(invalid(
                "workspace.repos.default_branch",
                &format!("'{}' is not a valid branch name", repo.default_branch),
            ));
        }
    }

    let primaries: Vec<&RepoSpec> = repos
        .iter()
        .filter(|r| r.role == RepoRole::Primary)
        .collect();
    if primaries.len() != 1 {
        return Err(invalid(
            "workspace.repos",
            &format!(
                "exactly one role: primary entry is required, found {}",
                primaries.len()
            ),
        ));
    }
    if !primaries[0].slug.eq_ignore_ascii_case(tracker_project_slug) {
        return Err(invalid(
            "workspace.repos",
            &format!(
                "primary slug '{}' must equal tracker.project_slug '{}'",
                primaries[0].slug, tracker_project_slug
            ),
        ));
    }

    for repo in repos {
        for dep in &repo.merge_after {
            if dep == &repo.alias {
                return Err(invalid(
                    "workspace.repos.merge_after",
                    &format!("'{}' depends on itself", repo.alias),
                ));
            }
            if !seen.contains(dep) {
                return Err(invalid(
                    "workspace.repos.merge_after",
                    &format!("'{}' references unknown alias '{dep}'", repo.alias),
                ));
            }
        }
    }
    merge_order(repos).map(|_| ()).map_err(|cycle| {
        invalid(
            "workspace.repos.merge_after",
            &format!("cycle detected involving '{cycle}'"),
        )
    })
}

/// Topologically sort repos so that dependencies (`merge_after`) come first.
/// Ties keep declaration order. Returns the alias that closes a cycle on error.
pub fn merge_order(repos: &[RepoSpec]) -> Result<Vec<String>, String> {
    let index: HashMap<&str, usize> = repos
        .iter()
        .enumerate()
        .map(|(i, r)| (r.alias.as_str(), i))
        .collect();
    let mut state = vec![0u8; repos.len()]; // 0 = new, 1 = visiting, 2 = done
    let mut out = Vec::with_capacity(repos.len());

    fn visit(
        i: usize,
        repos: &[RepoSpec],
        index: &HashMap<&str, usize>,
        state: &mut [u8],
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        match state[i] {
            2 => return Ok(()),
            1 => return Err(repos[i].alias.clone()),
            _ => {}
        }
        state[i] = 1;
        for dep in &repos[i].merge_after {
            if let Some(&j) = index.get(dep.as_str()) {
                visit(j, repos, index, state, out)?;
            }
        }
        state[i] = 2;
        out.push(repos[i].alias.clone());
        Ok(())
    }

    for i in 0..repos.len() {
        visit(i, repos, &index, &mut state, &mut out)?;
    }
    Ok(out)
}

fn is_valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && alias.len() <= 64
        && alias
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Conservative git ref check (subset of `git check-ref-format`).
pub(crate) fn is_safe_ref(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('-')
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.ends_with(".lock")
        && !name.contains("..")
        && !name.contains("@{")
        && !name.contains("//")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
}

/// Detect hook commands that clone into the workspace root (`git clone <url> .`
/// or `git clone <url>` followed by nothing that would land in cwd).
fn clones_into_workspace_root(script: &str) -> bool {
    script.lines().any(|line| {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(pos) = tokens
            .windows(2)
            .position(|w| w[0] == "git" && w[1] == "clone")
        else {
            return false;
        };
        let args: Vec<&str> = tokens[pos + 2..]
            .iter()
            .copied()
            .take_while(|t| !matches!(*t, "&&" | "||" | ";" | "|"))
            .collect();
        matches!(args.last(), Some(&".") | Some(&"./"))
    })
}

fn invalid(field: &str, detail: &str) -> ServiceConfigError {
    ServiceConfigError::InvalidValue {
        field: field.to_string(),
        detail: detail.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(yaml: &str) -> HashMap<String, YamlValue> {
        let value: YamlValue = serde_yaml::from_str(yaml).unwrap();
        let mut map = HashMap::new();
        if let YamlValue::Mapping(m) = value {
            for (k, v) in m {
                map.insert(k.as_str().unwrap().to_string(), v);
            }
        }
        map
    }

    const VALID: &str = r#"
repos:
  - alias: frontend
    slug: "zimei10525/jst-ai-studio"
    role: primary
    default_branch: dev
    mode: write
    merge_after: [backend]
  - alias: backend
    slug: "JstAi/jxy"
    default_branch: pre
"#;

    #[test]
    fn empty_or_missing_repos_is_single_repo_mode() {
        assert!(parse_repos(&HashMap::new(), "a/b", None)
            .unwrap()
            .is_empty());
        assert!(parse_repos(&section("repos: []"), "a/b", None)
            .unwrap()
            .is_empty());
        assert!(parse_repos(&section("repos:"), "a/b", None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn parses_valid_config_with_defaults() {
        let repos = parse_repos(&section(VALID), "zimei10525/jst-ai-studio", None).unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].role, RepoRole::Primary);
        assert!(repos[0].writable());
        assert_eq!(repos[1].mode, RepoMode::Read);
        assert_eq!(repos[1].checkout, CheckoutMode::Partial);
        assert_eq!(repos[1].role, RepoRole::Secondary);
        assert_eq!(merge_order(&repos).unwrap(), vec!["backend", "frontend"]);
    }

    #[test]
    fn primary_must_match_tracker_slug_case_insensitively() {
        assert!(parse_repos(&section(VALID), "ZIMEI10525/JST-AI-STUDIO", None).is_ok());
        let err = parse_repos(&section(VALID), "other/repo", None).unwrap_err();
        assert!(err.to_string().contains("tracker.project_slug"));
    }

    #[test]
    fn rejects_missing_or_multiple_primary() {
        let none = VALID.replace("role: primary", "role: secondary");
        assert!(parse_repos(&section(&none), "zimei10525/jst-ai-studio", None).is_err());
        let two = VALID.replace(
            "default_branch: pre",
            "default_branch: pre\n    role: primary",
        );
        assert!(parse_repos(&section(&two), "zimei10525/jst-ai-studio", None).is_err());
    }

    #[test]
    fn rejects_bad_alias_duplicate_and_reserved() {
        for bad in ["Backend", "back_end", "back end", "tmp"] {
            let y = VALID.replace("alias: backend", &format!("alias: \"{bad}\""));
            let y = y.replace("merge_after: [backend]", "merge_after: []");
            assert!(
                parse_repos(&section(&y), "zimei10525/jst-ai-studio", None).is_err(),
                "alias {bad} should be rejected"
            );
        }
        let dup = VALID
            .replace("alias: backend", "alias: frontend")
            .replace("merge_after: [backend]", "");
        assert!(parse_repos(&section(&dup), "zimei10525/jst-ai-studio", None).is_err());
    }

    #[test]
    fn rejects_unknown_dependency_self_and_cycle() {
        let unknown = VALID.replace("merge_after: [backend]", "merge_after: [nope]");
        assert!(parse_repos(&section(&unknown), "zimei10525/jst-ai-studio", None).is_err());
        let selfdep = VALID.replace("merge_after: [backend]", "merge_after: [frontend]");
        assert!(parse_repos(&section(&selfdep), "zimei10525/jst-ai-studio", None).is_err());
        let cycle = VALID.replace(
            "default_branch: pre",
            "default_branch: pre\n    merge_after: [frontend]",
        );
        let err = parse_repos(&section(&cycle), "zimei10525/jst-ai-studio", None).unwrap_err();
        assert!(err.to_string().contains("cycle"));
    }

    #[test]
    fn rejects_bad_slug_branch_and_enum_values() {
        let slug = VALID.replace("\"JstAi/jxy\"", "\"jxy\"");
        assert!(parse_repos(&section(&slug), "zimei10525/jst-ai-studio", None).is_err());
        let url = VALID.replace("\"JstAi/jxy\"", "\"http://h/JstAi/jxy\"");
        assert!(parse_repos(&section(&url), "zimei10525/jst-ai-studio", None).is_err());
        let branch = VALID.replace("default_branch: pre", "default_branch: \"-x\"");
        assert!(parse_repos(&section(&branch), "zimei10525/jst-ai-studio", None).is_err());
        let mode = VALID.replace("mode: write", "mode: admin");
        assert!(parse_repos(&section(&mode), "zimei10525/jst-ai-studio", None).is_err());
        let shallow = VALID.replace(
            "default_branch: pre",
            "default_branch: pre\n    checkout: shallow",
        );
        assert!(parse_repos(&section(&shallow), "zimei10525/jst-ai-studio", None).is_err());
    }

    #[test]
    fn rejects_after_create_clone_into_root() {
        let s = section(VALID);
        let slug = "zimei10525/jst-ai-studio";
        assert!(parse_repos(&s, slug, Some("git clone http://h/a/b.git .")).is_err());
        assert!(parse_repos(&s, slug, Some("cd x && git clone --depth 1 url . && ls")).is_err());
        assert!(parse_repos(&s, slug, Some("git clone url extra-dir")).is_ok());
        assert!(parse_repos(&s, slug, Some("npm ci")).is_ok());
    }

    #[test]
    fn primary_key_normalizes() {
        assert_eq!(primary_key("jst-ai-studio"), "jst-ai-studio");
        assert_eq!(primary_key("My_Repo.JS"), "my-repo-js");
    }
}
