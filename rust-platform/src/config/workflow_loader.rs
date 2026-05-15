//! Workflow Loader — parses WORKFLOW.md into config + prompt template.
//!
//! WORKFLOW.md is a Markdown file with OPTIONAL YAML front matter delimited by `---`.
//! The front matter MUST decode to a map/object; non-map YAML is an error.
//! The remaining body (trimmed) becomes the prompt template.
//!
//! SPEC reference: Section 5.1-5.2

use std::collections::HashMap;
use std::path::Path;

use serde_yaml::Value as YamlValue;
use thiserror::Error;

/// Parsed WORKFLOW.md payload (SPEC Section 4.1.2).
#[derive(Debug, Clone)]
pub struct WorkflowDefinition {
    /// YAML front matter root object.
    pub config: HashMap<String, YamlValue>,
    /// Markdown body after front matter, trimmed.
    pub prompt_template: String,
}

/// Errors that can occur when loading/parsing a workflow file.
#[derive(Debug, Error)]
pub enum WorkflowLoadError {
    #[error("workflow file not found: {path}")]
    MissingWorkflowFile { path: std::path::PathBuf },

    #[error("workflow YAML parse error: {source}")]
    WorkflowParseError {
        #[source]
        source: serde_yaml::Error,
    },

    #[error("workflow front matter must be a YAML map/object")]
    WorkflowFrontMatterNotAMap,

    #[error("I/O error reading workflow file: {source}")]
    IoError {
        #[source]
        source: std::io::Error,
    },
}

/// Load and parse a WORKFLOW.md file from disk.
///
/// Returns a `WorkflowDefinition` containing the parsed config map and prompt template.
///
/// # Errors
///
/// - `MissingWorkflowFile` if the file does not exist or cannot be read.
/// - `WorkflowParseError` if the YAML front matter is invalid.
/// - `WorkflowFrontMatterNotAMap` if the front matter is valid YAML but not a map.
pub fn load_workflow(path: &Path) -> Result<WorkflowDefinition, WorkflowLoadError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            WorkflowLoadError::MissingWorkflowFile {
                path: path.to_path_buf(),
            }
        } else {
            WorkflowLoadError::IoError { source: e }
        }
    })?;

    parse_workflow(&content)
}

/// Parse workflow content from a string (used for hot reload and testing).
///
/// Follows the parsing algorithm from SPEC Section 5.2:
/// 1. If content starts with `---\n`, find the closing `---\n` and parse the
///    middle as YAML. The remainder is the prompt body.
/// 2. Otherwise, the entire content is the prompt body with an empty config.
pub fn parse_workflow(content: &str) -> Result<WorkflowDefinition, WorkflowLoadError> {
    // Check for front matter delimiter
    if content.starts_with("---\n") || content.starts_with("---\r\n") {
        // Find the closing delimiter
        let after_first = if content.starts_with("---\r\n") {
            5
        } else {
            4
        };

        let rest = &content[after_first..];

        // Look for the closing `---` on its own line
        let closing_pos = find_closing_delimiter(rest);

        match closing_pos {
            Some(pos) => {
                let yaml_str = &rest[..pos];
                let body_start = pos + 3; // skip "---"
                // Skip the newline after closing delimiter
                let body_rest = &rest[body_start..];
                let body = if body_rest.starts_with('\n') {
                    &body_rest[1..]
                } else if body_rest.starts_with("\r\n") {
                    &body_rest[2..]
                } else {
                    body_rest
                };

                let config = parse_yaml_front_matter(yaml_str)?;
                let prompt_template = body.trim().to_string();

                Ok(WorkflowDefinition {
                    config,
                    prompt_template,
                })
            }
            None => {
                // No closing delimiter found — treat entire content as prompt body
                Ok(WorkflowDefinition {
                    config: HashMap::new(),
                    prompt_template: content.trim().to_string(),
                })
            }
        }
    } else {
        // No front matter — entire content is prompt body
        Ok(WorkflowDefinition {
            config: HashMap::new(),
            prompt_template: content.trim().to_string(),
        })
    }
}

/// Find the position of the closing `---` delimiter in the remaining content.
/// The delimiter must appear at the start of a line.
fn find_closing_delimiter(content: &str) -> Option<usize> {
    // Look for "\n---\n" or the content starting with "---\n"
    if content.starts_with("---\n") || content.starts_with("---\r\n") {
        return Some(0);
    }

    if let Some(pos) = content.find("\n---\n") {
        return Some(pos + 1); // +1 to skip the preceding newline
    }

    if let Some(pos) = content.find("\n---\r\n") {
        return Some(pos + 1);
    }

    // Also handle case where --- is at the very end (no trailing newline)
    if content.ends_with("\n---") {
        return Some(content.len() - 3);
    }

    None
}

/// Parse YAML front matter string into a HashMap.
/// Returns an error if the YAML is invalid or not a map.
fn parse_yaml_front_matter(yaml_str: &str) -> Result<HashMap<String, YamlValue>, WorkflowLoadError> {
    // Empty front matter is valid — returns empty map
    let trimmed = yaml_str.trim();
    if trimmed.is_empty() {
        return Ok(HashMap::new());
    }

    let value: YamlValue =
        serde_yaml::from_str(trimmed).map_err(|e| WorkflowLoadError::WorkflowParseError { source: e })?;

    match value {
        YamlValue::Mapping(mapping) => {
            let mut map = HashMap::new();
            for (k, v) in mapping {
                if let YamlValue::String(key) = k {
                    map.insert(key, v);
                } else {
                    // Non-string keys: convert to string representation
                    let key_str = format!("{:?}", k);
                    map.insert(key_str, v);
                }
            }
            Ok(map)
        }
        YamlValue::Null => {
            // Empty YAML (e.g., just comments) parses as Null — treat as empty map
            Ok(HashMap::new())
        }
        _ => Err(WorkflowLoadError::WorkflowFrontMatterNotAMap),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_workflow_with_front_matter() {
        let content = "---\ntracker:\n  kind: linear\n  project_slug: my-project\npolling:\n  interval_ms: 5000\n---\nYou are working on {{ issue.title }}.\n";

        let result = parse_workflow(content).unwrap();

        assert!(result.config.contains_key("tracker"));
        assert!(result.config.contains_key("polling"));
        assert_eq!(
            result.prompt_template,
            "You are working on {{ issue.title }}."
        );
    }

    #[test]
    fn test_parse_workflow_without_front_matter() {
        let content = "Just a prompt template with no config.\n";

        let result = parse_workflow(content).unwrap();

        assert!(result.config.is_empty());
        assert_eq!(
            result.prompt_template,
            "Just a prompt template with no config."
        );
    }

    #[test]
    fn test_parse_workflow_empty_front_matter() {
        let content = "---\n---\nPrompt body here.\n";

        let result = parse_workflow(content).unwrap();

        assert!(result.config.is_empty());
        assert_eq!(result.prompt_template, "Prompt body here.");
    }

    #[test]
    fn test_parse_workflow_non_map_front_matter() {
        let content = "---\n- item1\n- item2\n---\nPrompt body.\n";

        let result = parse_workflow(content);

        assert!(matches!(
            result,
            Err(WorkflowLoadError::WorkflowFrontMatterNotAMap)
        ));
    }

    #[test]
    fn test_parse_workflow_invalid_yaml() {
        let content = "---\n: invalid: yaml: [[\n---\nPrompt body.\n";

        let result = parse_workflow(content);

        assert!(matches!(
            result,
            Err(WorkflowLoadError::WorkflowParseError { .. })
        ));
    }

    #[test]
    fn test_parse_workflow_empty_prompt_body() {
        let content = "---\ntracker:\n  kind: linear\n---\n";

        let result = parse_workflow(content).unwrap();

        assert!(result.config.contains_key("tracker"));
        assert_eq!(result.prompt_template, "");
    }

    #[test]
    fn test_parse_workflow_no_closing_delimiter() {
        // If there's no closing ---, treat entire content as prompt
        let content = "---\nthis looks like front matter but has no closing\n";

        let result = parse_workflow(content).unwrap();

        assert!(result.config.is_empty());
        assert_eq!(
            result.prompt_template,
            "---\nthis looks like front matter but has no closing"
        );
    }

    #[test]
    fn test_load_workflow_missing_file() {
        let result = load_workflow(Path::new("/nonexistent/path/WORKFLOW.md"));

        assert!(matches!(
            result,
            Err(WorkflowLoadError::MissingWorkflowFile { .. })
        ));
    }

    #[test]
    fn test_parse_workflow_scalar_front_matter() {
        let content = "---\njust a string\n---\nPrompt.\n";

        let result = parse_workflow(content);

        assert!(matches!(
            result,
            Err(WorkflowLoadError::WorkflowFrontMatterNotAMap)
        ));
    }

    #[test]
    fn test_parse_workflow_multiline_prompt() {
        let content = "---\ntracker:\n  kind: linear\n---\nLine 1.\n\nLine 2.\n\nLine 3.\n";

        let result = parse_workflow(content).unwrap();

        assert_eq!(result.prompt_template, "Line 1.\n\nLine 2.\n\nLine 3.");
    }
}
