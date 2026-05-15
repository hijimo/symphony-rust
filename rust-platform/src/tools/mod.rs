//! Tools module — provides controlled interfaces for agent-initiated platform operations.
//!
//! Contains:
//! - `platform_api`: Controlled interface for GitHub/GitLab operations
//! - `linear_graphql`: Dynamic tool for executing GraphQL against Linear

pub mod linear_graphql;
pub mod platform_api;

pub use linear_graphql::{DynamicTool, LinearGraphqlTool, ToolRegistry, ToolResult, ToolSpec};
pub use platform_api::{
    extract_issue_id, extract_pr_params, extract_string, sanitize_value, PlatformApiTool,
};
