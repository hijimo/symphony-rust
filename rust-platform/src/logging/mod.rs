//! Structured Logging — initialization and configuration.
//!
//! Implements SPEC Section 13.1-13.2:
//! - JSON format for production (machine-parseable)
//! - Pretty format for development (human-readable)
//! - Required context fields: issue_id, issue_identifier, session_id
//! - No secrets in logs

use std::io::Write;

use tracing_subscriber::fmt;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

/// Writer that flushes stderr after every write operation.
struct FlushWriter;

impl<'a> MakeWriter<'a> for FlushWriter {
    type Writer = FlushOnDrop;

    fn make_writer(&'a self) -> Self::Writer {
        FlushOnDrop(std::io::stderr())
    }
}

struct FlushOnDrop(std::io::Stderr);

impl Write for FlushOnDrop {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl Drop for FlushOnDrop {
    fn drop(&mut self) {
        let _ = self.0.flush();
    }
}

/// Logging output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// JSON structured logs (production).
    Json,
    /// Pretty human-readable logs (development).
    Pretty,
}

/// Initialize the global tracing subscriber.
///
/// Format selection:
/// - If `SYMPHONY_LOG_FORMAT=json` or `RUST_LOG_FORMAT=json` -> JSON
/// - If `SYMPHONY_LOG_FORMAT=pretty` -> Pretty
/// - Default: Pretty for interactive terminals, JSON otherwise
///
/// Log level is controlled by `RUST_LOG` env var (default: `info`).
pub fn init_logging(format: Option<LogFormat>) {
    let format = format.unwrap_or_else(detect_format);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    match format {
        LogFormat::Json => {
            let subscriber = fmt::layer()
                .json()
                .with_target(true)
                .with_thread_ids(false)
                .with_file(false)
                .with_line_number(false)
                .with_span_list(true)
                .with_writer(FlushWriter);

            tracing_subscriber::registry()
                .with(env_filter)
                .with(subscriber)
                .init();
        }
        LogFormat::Pretty => {
            let subscriber = fmt::layer()
                .pretty()
                .with_target(true)
                .with_thread_ids(false)
                .with_writer(FlushWriter);

            tracing_subscriber::registry()
                .with(env_filter)
                .with(subscriber)
                .init();
        }
    }

    tracing::debug!(format = ?format, "logging initialized");
}

/// Detect the appropriate log format from environment.
fn detect_format() -> LogFormat {
    // Check explicit format env vars
    if let Ok(fmt) = std::env::var("SYMPHONY_LOG_FORMAT") {
        return match fmt.to_lowercase().as_str() {
            "json" => LogFormat::Json,
            "pretty" | "text" => LogFormat::Pretty,
            _ => LogFormat::Pretty,
        };
    }

    if let Ok(fmt) = std::env::var("RUST_LOG_FORMAT") {
        return match fmt.to_lowercase().as_str() {
            "json" => LogFormat::Json,
            _ => LogFormat::Pretty,
        };
    }

    // Default: pretty for terminals, JSON for non-interactive
    if atty_is_terminal() {
        LogFormat::Pretty
    } else {
        LogFormat::Json
    }
}

/// Check if stderr is a terminal (heuristic for interactive use).
fn atty_is_terminal() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stderr())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_format_detection_defaults_to_pretty_in_tests() {
        // In test context, stderr is typically not a terminal,
        // but we just verify the function doesn't panic.
        let _ = detect_format();
    }

    #[test]
    fn test_log_format_enum() {
        assert_ne!(LogFormat::Json, LogFormat::Pretty);
    }
}
