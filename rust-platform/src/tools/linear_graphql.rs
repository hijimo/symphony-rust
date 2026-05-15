//! `linear_graphql` dynamic tool — execute GraphQL queries against Linear.
//!
//! Implements SPEC Section 10.5 (linear_graphql extension contract):
//! - Execute raw GraphQL query/mutation using Symphony's configured Linear auth
//! - Validate single operation per query
//! - Return structured tool output for in-session model inspection

use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Tool specification advertised to the app-server session at startup.
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Result of a dynamic tool execution.
#[derive(Debug, Clone, Serialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: Value,
}

/// Trait for dynamic tools that can be registered and invoked by the agent.
#[async_trait]
pub trait DynamicTool: Send + Sync {
    /// Return the tool specification for session advertisement.
    fn spec(&self) -> ToolSpec;

    /// Execute the tool with the given input.
    async fn execute(&self, input: Value) -> ToolResult;
}

/// Registry of available dynamic tools.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn DynamicTool>>,
}

impl ToolRegistry {
    /// Create an empty tool registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a dynamic tool.
    pub fn register(&mut self, tool: Arc<dyn DynamicTool>) {
        let spec = tool.spec();
        self.tools.insert(spec.name.clone(), tool);
    }

    /// Get all tool specifications for session advertisement.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|t| t.spec()).collect()
    }

    /// Handle a tool call by name. Returns a failure result for unknown tools.
    pub async fn handle_call(&self, name: &str, input: Value) -> ToolResult {
        match self.tools.get(name) {
            Some(tool) => tool.execute(input).await,
            None => ToolResult {
                success: false,
                output: json!({"error": format!("unsupported tool: {}", name)}),
            },
        }
    }

    /// Check if a tool is registered.
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Linear GraphQL dynamic tool.
///
/// Executes raw GraphQL queries/mutations against Linear using the configured
/// tracker authentication. Only available when `tracker.kind == "linear"`.
pub struct LinearGraphqlTool {
    endpoint: String,
    api_key: String,
    http: Client,
}

impl LinearGraphqlTool {
    /// Create a new LinearGraphqlTool with the given Linear configuration.
    pub fn new(endpoint: String, api_key: String) -> Result<Self, String> {
        if api_key.is_empty() {
            return Err("Linear API key is required for linear_graphql tool".into());
        }

        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("failed to create HTTP client: {}", e))?;

        Ok(Self {
            endpoint,
            api_key,
            http,
        })
    }
}

#[async_trait]
impl DynamicTool for LinearGraphqlTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "linear_graphql".into(),
            description: "Execute a GraphQL query or mutation against Linear using Symphony's configured tracker auth.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "A single GraphQL query or mutation document"
                    },
                    "variables": {
                        "type": "object",
                        "description": "Optional GraphQL variables object"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn execute(&self, input: Value) -> ToolResult {
        // 1. Extract and validate query
        let query = match input.get("query").and_then(|q| q.as_str()) {
            Some(q) if !q.trim().is_empty() => q,
            _ => {
                return ToolResult {
                    success: false,
                    output: json!({"error": "query must be a non-empty string"}),
                };
            }
        };

        // 2. Validate single operation (basic heuristic: count top-level query/mutation/subscription keywords)
        if !validate_single_operation(query) {
            return ToolResult {
                success: false,
                output: json!({"error": "query must contain exactly one GraphQL operation"}),
            };
        }

        // 3. Extract optional variables
        let variables = input
            .get("variables")
            .cloned()
            .unwrap_or(Value::Object(serde_json::Map::new()));

        if !variables.is_object() {
            return ToolResult {
                success: false,
                output: json!({"error": "variables must be a JSON object when provided"}),
            };
        }

        // 4. Execute the GraphQL request
        let body = json!({
            "query": query,
            "variables": variables,
        });

        let response = match self
            .http
            .post(&self.endpoint)
            .header("Authorization", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                return ToolResult {
                    success: false,
                    output: json!({"error": format!("transport failure: {}", e)}),
                };
            }
        };

        let status = response.status().as_u16();
        if status != 200 {
            let body_text = response.text().await.unwrap_or_default();
            return ToolResult {
                success: false,
                output: json!({
                    "error": format!("HTTP {}", status),
                    "body": body_text,
                }),
            };
        }

        // 5. Parse response
        let json_response: Value = match response.json().await {
            Ok(v) => v,
            Err(e) => {
                return ToolResult {
                    success: false,
                    output: json!({"error": format!("failed to parse response: {}", e)}),
                };
            }
        };

        // 6. Check for GraphQL errors
        let has_errors = json_response
            .get("errors")
            .and_then(|e| e.as_array())
            .map(|arr| !arr.is_empty())
            .unwrap_or(false);

        if has_errors {
            // success=false but preserve the full response body for debugging
            ToolResult {
                success: false,
                output: json_response,
            }
        } else {
            ToolResult {
                success: true,
                output: json_response,
            }
        }
    }
}

/// Validate that a GraphQL document contains exactly one operation.
///
/// This is a heuristic check that counts top-level operation keywords
/// (query, mutation, subscription) that appear at the start of a line
/// or after a closing brace.
fn validate_single_operation(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Count operation definitions by looking for top-level keywords
    // A more robust approach would use a GraphQL parser, but this covers
    // the common cases per SPEC requirements.
    let mut operation_count = 0;
    let mut depth: u32 = 0;

    for line in trimmed.lines() {
        let line = line.trim();

        // Track brace depth
        for ch in line.chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }

        // Only count operations at depth 0 (top level)
        if depth == 0 || (depth == 1 && line.contains('{')) {
            let lower = line.to_lowercase();
            if lower.starts_with("query ")
                || lower.starts_with("query{")
                || lower.starts_with("mutation ")
                || lower.starts_with("mutation{")
                || lower.starts_with("subscription ")
                || lower.starts_with("subscription{")
            {
                operation_count += 1;
            }
            // Anonymous query (starts with {)
            if line.starts_with('{') && operation_count == 0 {
                operation_count += 1;
            }
        }
    }

    // If no explicit operation keyword found but query is non-empty,
    // treat it as a single anonymous operation
    if operation_count == 0 && !trimmed.is_empty() {
        operation_count = 1;
    }

    operation_count == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_single_operation_query() {
        assert!(validate_single_operation("query { users { id } }"));
        assert!(validate_single_operation(
            "query GetUser($id: ID!) { user(id: $id) { name } }"
        ));
    }

    #[test]
    fn test_validate_single_operation_mutation() {
        assert!(validate_single_operation(
            "mutation { createUser(name: \"test\") { id } }"
        ));
    }

    #[test]
    fn test_validate_single_operation_anonymous() {
        assert!(validate_single_operation("{ users { id name } }"));
    }

    #[test]
    fn test_validate_single_operation_empty() {
        assert!(!validate_single_operation(""));
        assert!(!validate_single_operation("   "));
    }

    #[test]
    fn test_tool_registry_unknown_tool() {
        let registry = ToolRegistry::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(registry.handle_call("unknown_tool", json!({})));
        assert!(!result.success);
        assert!(result.output["error"]
            .as_str()
            .unwrap()
            .contains("unsupported tool"));
    }

    #[test]
    fn test_tool_registry_specs() {
        let registry = ToolRegistry::new();
        assert!(registry.specs().is_empty());
    }

    #[test]
    fn test_linear_graphql_tool_spec() {
        let tool =
            LinearGraphqlTool::new("https://api.linear.app/graphql".into(), "test-key".into())
                .unwrap();
        let spec = tool.spec();
        assert_eq!(spec.name, "linear_graphql");
        assert!(spec.parameters["required"]
            .as_array()
            .unwrap()
            .contains(&json!("query")));
    }

    #[test]
    fn test_linear_graphql_tool_missing_api_key() {
        let result = LinearGraphqlTool::new("https://api.linear.app/graphql".into(), "".into());
        assert!(result.is_err());
    }
}
