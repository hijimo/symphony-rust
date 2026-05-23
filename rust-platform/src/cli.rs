//! CLI argument parsing for Symphony.
//!
//! Implements SPEC Section 17.7:
//! - Optional positional argument for WORKFLOW.md path
//! - --port flag to enable HTTP server extension
//! - Startup flow: parse args -> init logging -> load workflow -> validate -> run

use std::path::PathBuf;

use clap::Parser;

/// Symphony — orchestrate coding agents for project work.
#[derive(Parser, Debug)]
#[command(
    name = "symphony",
    about = "Orchestrate coding agents to get project work done",
    version
)]
pub struct Cli {
    /// Path to WORKFLOW.md (default: ./WORKFLOW.md in current directory).
    #[arg(value_name = "WORKFLOW_PATH")]
    pub workflow_path: Option<PathBuf>,

    /// HTTP server port (overrides server.port in WORKFLOW.md).
    /// Enables the HTTP server extension when provided.
    #[arg(long)]
    pub port: Option<u16>,
}

impl Cli {
    /// Resolve the effective workflow path.
    ///
    /// Returns the explicitly provided path or falls back to `./WORKFLOW.md`.
    pub fn effective_workflow_path(&self) -> PathBuf {
        self.workflow_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("./WORKFLOW.md"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_workflow_path() {
        let cli = Cli {
            workflow_path: None,
            port: None,
        };
        assert_eq!(
            cli.effective_workflow_path(),
            PathBuf::from("./WORKFLOW.md")
        );
    }

    #[test]
    fn test_explicit_workflow_path() {
        let cli = Cli {
            workflow_path: Some(PathBuf::from("/etc/symphony/WORKFLOW.md")),
            port: None,
        };
        assert_eq!(
            cli.effective_workflow_path(),
            PathBuf::from("/etc/symphony/WORKFLOW.md")
        );
    }

    #[test]
    fn test_port_override() {
        let cli = Cli {
            workflow_path: None,
            port: Some(8080),
        };
        assert_eq!(cli.port, Some(8080));
    }
}
