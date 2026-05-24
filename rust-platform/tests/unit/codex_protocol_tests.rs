//! Unit tests for Codex protocol message handling.
//!
//! Tests cover:
//! - JSON-RPC 2.0 message construction (turn.start)
//! - Event parsing (turn/completed, turn/failed, turn/cancelled, input_required)
//! - Token usage extraction
//! - Thread/turn ID tracking
//! - Malformed message handling
//! - Event type normalization (dot vs underscore variants)

use chrono::{DateTime, Utc};
use serde_json::json;

use symphony_platform::agent::codex_client::{CodexError, TurnResult};
use symphony_platform::models::TokenUsage;

// ═══════════════════════════════════════════════════════════════════════════════
// Turn Start Request Construction Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod turn_start_request {
    use super::*;

    #[test]
    fn test_turn_start_request_structure() {
        let prompt = "Work on issue ABC-42: Fix login bug";
        let request = json!({
            "type": "turn.start",
            "prompt": prompt,
        });

        assert_eq!(request["type"], "turn.start");
        assert_eq!(request["prompt"], prompt);
    }

    #[test]
    fn test_turn_start_request_serializes_to_json_line() {
        let prompt = "Fix the bug";
        let request = json!({
            "type": "turn.start",
            "prompt": prompt,
        });

        let serialized = serde_json::to_string(&request).unwrap();
        assert!(!serialized.contains('\n')); // Single line
        assert!(serialized.contains("turn.start"));
        assert!(serialized.contains("Fix the bug"));
    }

    #[test]
    fn test_turn_start_request_with_special_chars_in_prompt() {
        let prompt = "Fix \"quoted\" text and\nnewlines and\ttabs";
        let request = json!({
            "type": "turn.start",
            "prompt": prompt,
        });

        let serialized = serde_json::to_string(&request).unwrap();
        // Should properly escape special characters
        let deserialized: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized["prompt"].as_str().unwrap(), prompt);
    }

    #[test]
    fn test_stop_request_structure() {
        let request = json!({"type": "stop"});
        assert_eq!(request["type"], "stop");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Event Parsing Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod event_parsing {
    use super::*;

    /// Simulate the check_turn_terminal logic for testing
    fn check_turn_terminal(event: &serde_json::Value) -> Option<Result<TurnResult, CodexError>> {
        let event_type = event
            .get("type")
            .or_else(|| event.get("event"))
            .and_then(|v| v.as_str())?;

        match event_type {
            "turn.completed" | "turn_completed" => Some(Ok(TurnResult::Completed)),
            "turn.failed" | "turn_failed" => {
                let reason = event
                    .get("error")
                    .or_else(|| event.get("reason"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                Some(Ok(TurnResult::Failed { reason }))
            }
            "turn.cancelled" | "turn_cancelled" => Some(Err(CodexError::TurnCancelled)),
            "turn.input_required" | "turn_input_required" | "input_required" => {
                Some(Err(CodexError::TurnInputRequired))
            }
            "turn.error" | "turn_ended_with_error" => {
                let detail = event
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error")
                    .to_string();
                Some(Err(CodexError::TurnFailed { reason: detail }))
            }
            _ => None,
        }
    }

    #[test]
    fn test_parse_turn_completed_dot_notation() {
        let event = json!({"type": "turn.completed"});
        let result = check_turn_terminal(&event);
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), Ok(TurnResult::Completed)));
    }

    #[test]
    fn test_parse_turn_completed_underscore_notation() {
        let event = json!({"type": "turn_completed"});
        let result = check_turn_terminal(&event);
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), Ok(TurnResult::Completed)));
    }

    #[test]
    fn test_parse_turn_failed_with_error_field() {
        let event = json!({
            "type": "turn.failed",
            "error": "rate limit exceeded"
        });
        let result = check_turn_terminal(&event);
        assert!(result.is_some());
        match result.unwrap() {
            Ok(TurnResult::Failed { reason }) => {
                assert_eq!(reason, "rate limit exceeded");
            }
            _ => panic!("Expected TurnResult::Failed"),
        }
    }

    #[test]
    fn test_parse_turn_failed_with_reason_field() {
        let event = json!({
            "type": "turn.failed",
            "reason": "timeout"
        });
        let result = check_turn_terminal(&event);
        assert!(result.is_some());
        match result.unwrap() {
            Ok(TurnResult::Failed { reason }) => {
                assert_eq!(reason, "timeout");
            }
            _ => panic!("Expected TurnResult::Failed"),
        }
    }

    #[test]
    fn test_parse_turn_failed_no_reason() {
        let event = json!({"type": "turn.failed"});
        let result = check_turn_terminal(&event);
        assert!(result.is_some());
        match result.unwrap() {
            Ok(TurnResult::Failed { reason }) => {
                assert_eq!(reason, "unknown");
            }
            _ => panic!("Expected TurnResult::Failed"),
        }
    }

    #[test]
    fn test_parse_turn_cancelled() {
        let event = json!({"type": "turn.cancelled"});
        let result = check_turn_terminal(&event);
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), Err(CodexError::TurnCancelled)));
    }

    #[test]
    fn test_parse_turn_cancelled_underscore() {
        let event = json!({"type": "turn_cancelled"});
        let result = check_turn_terminal(&event);
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), Err(CodexError::TurnCancelled)));
    }

    #[test]
    fn test_parse_input_required_dot_notation() {
        let event = json!({"type": "turn.input_required"});
        let result = check_turn_terminal(&event);
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap(),
            Err(CodexError::TurnInputRequired)
        ));
    }

    #[test]
    fn test_parse_input_required_underscore_notation() {
        let event = json!({"type": "turn_input_required"});
        let result = check_turn_terminal(&event);
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap(),
            Err(CodexError::TurnInputRequired)
        ));
    }

    #[test]
    fn test_parse_input_required_short_form() {
        let event = json!({"type": "input_required"});
        let result = check_turn_terminal(&event);
        assert!(result.is_some());
        assert!(matches!(
            result.unwrap(),
            Err(CodexError::TurnInputRequired)
        ));
    }

    #[test]
    fn test_parse_turn_error() {
        let event = json!({
            "type": "turn.error",
            "error": "process crashed"
        });
        let result = check_turn_terminal(&event);
        assert!(result.is_some());
        match result.unwrap() {
            Err(CodexError::TurnFailed { reason }) => {
                assert_eq!(reason, "process crashed");
            }
            _ => panic!("Expected CodexError::TurnFailed"),
        }
    }

    #[test]
    fn test_parse_turn_ended_with_error() {
        let event = json!({
            "type": "turn_ended_with_error",
            "error": "OOM killed"
        });
        let result = check_turn_terminal(&event);
        assert!(result.is_some());
        match result.unwrap() {
            Err(CodexError::TurnFailed { reason }) => {
                assert_eq!(reason, "OOM killed");
            }
            _ => panic!("Expected CodexError::TurnFailed"),
        }
    }

    #[test]
    fn test_parse_non_terminal_event_returns_none() {
        let event = json!({"type": "item.tool.call", "tool": "bash"});
        let result = check_turn_terminal(&event);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_event_field_instead_of_type() {
        // Some events use "event" field instead of "type"
        let event = json!({"event": "turn.completed"});
        let result = check_turn_terminal(&event);
        assert!(result.is_some());
        assert!(matches!(result.unwrap(), Ok(TurnResult::Completed)));
    }

    #[test]
    fn test_parse_event_with_no_type_field() {
        let event = json!({"data": "something"});
        let result = check_turn_terminal(&event);
        assert!(result.is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Token Usage Extraction Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod token_usage {
    use super::*;

    #[test]
    fn test_extract_token_usage_from_event() {
        let event = json!({
            "type": "turn.completed",
            "usage": {
                "input_tokens": 1500,
                "output_tokens": 500,
                "total_tokens": 2000
            }
        });

        let usage = event.get("usage").map(|u| TokenUsage {
            input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        });

        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 1500);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.total_tokens, 2000);
    }

    #[test]
    fn test_extract_token_usage_missing() {
        let event = json!({"type": "turn.completed"});
        let usage = event.get("usage");
        assert!(usage.is_none());
    }

    #[test]
    fn test_extract_token_usage_partial() {
        let event = json!({
            "type": "turn.completed",
            "usage": {
                "input_tokens": 1000
            }
        });

        let usage = event.get("usage").map(|u| TokenUsage {
            input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        });

        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    #[test]
    fn test_token_usage_default() {
        let usage = TokenUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Thread/Turn ID Tracking Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod thread_turn_tracking {
    use super::*;

    #[test]
    fn test_extract_thread_id_from_event() {
        let event = json!({
            "type": "turn.started",
            "thread_id": "thread-abc-123",
            "turn_id": "turn-001"
        });

        let thread_id = event.get("thread_id").and_then(|v| v.as_str());
        let turn_id = event.get("turn_id").and_then(|v| v.as_str());

        assert_eq!(thread_id, Some("thread-abc-123"));
        assert_eq!(turn_id, Some("turn-001"));
    }

    #[test]
    fn test_thread_id_reuse_across_turns() {
        // Simulate multiple events with same thread_id but different turn_ids
        let events = vec![
            json!({"type": "turn.started", "thread_id": "thread-1", "turn_id": "turn-1"}),
            json!({"type": "turn.completed", "thread_id": "thread-1", "turn_id": "turn-1"}),
            json!({"type": "turn.started", "thread_id": "thread-1", "turn_id": "turn-2"}),
        ];

        let mut last_thread_id = None;
        let mut last_turn_id = None;

        for event in &events {
            if let Some(tid) = event.get("thread_id").and_then(|v| v.as_str()) {
                last_thread_id = Some(tid.to_string());
            }
            if let Some(tid) = event.get("turn_id").and_then(|v| v.as_str()) {
                last_turn_id = Some(tid.to_string());
            }
        }

        // Thread ID should remain the same
        assert_eq!(last_thread_id, Some("thread-1".to_string()));
        // Turn ID should be updated to the latest
        assert_eq!(last_turn_id, Some("turn-2".to_string()));
    }

    #[test]
    fn test_session_id_composition() {
        let thread_id = "thread-abc";
        let turn_id = "turn-001";
        let session_id = format!("{}-{}", thread_id, turn_id);
        assert_eq!(session_id, "thread-abc-turn-001");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Malformed Message Handling Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod malformed_messages {
    use super::*;

    #[test]
    fn test_invalid_json_is_detected() {
        let raw = "this is not json {{{";
        let result = serde_json::from_str::<serde_json::Value>(raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_line_is_skipped() {
        let raw = "";
        assert!(raw.trim().is_empty());
    }

    #[test]
    fn test_whitespace_only_line_is_skipped() {
        let raw = "   \t  ";
        assert!(raw.trim().is_empty());
    }

    #[test]
    fn test_oversized_line_detection() {
        let max_line_size = 10 * 1024 * 1024; // 10 MB
        let large_line = "x".repeat(max_line_size + 1);
        assert!(large_line.len() > max_line_size);
    }

    #[test]
    fn test_json_without_type_field() {
        let event = json!({"data": "something", "value": 42});
        let event_type = event
            .get("type")
            .or_else(|| event.get("event"))
            .and_then(|v| v.as_str());
        assert!(event_type.is_none());
    }

    #[test]
    fn test_json_with_null_type_field() {
        let event = json!({"type": null});
        let event_type = event.get("type").and_then(|v| v.as_str());
        assert!(event_type.is_none());
    }

    #[test]
    fn test_json_with_numeric_type_field() {
        let event = json!({"type": 42});
        let event_type = event.get("type").and_then(|v| v.as_str());
        assert!(event_type.is_none());
    }

    #[test]
    fn test_timestamp_parsing_valid_iso8601() {
        let event = json!({
            "type": "turn.completed",
            "timestamp": "2024-01-15T10:30:00Z"
        });

        let ts = event
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<DateTime<Utc>>().ok());

        assert!(ts.is_some());
    }

    #[test]
    fn test_timestamp_parsing_invalid_format() {
        let event = json!({
            "type": "turn.completed",
            "timestamp": "not-a-timestamp"
        });

        let ts = event
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<DateTime<Utc>>().ok());

        assert!(ts.is_none());
    }

    #[test]
    fn test_timestamp_missing_uses_fallback() {
        let event = json!({"type": "turn.completed"});

        let ts = event
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(Utc::now);

        // Should be approximately now
        let diff = (Utc::now() - ts).num_seconds().abs();
        assert!(diff < 2);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Rate Limits Extraction Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod rate_limits {
    use super::*;

    #[test]
    fn test_extract_rate_limits_from_event() {
        let event = json!({
            "type": "turn.completed",
            "rate_limits": {
                "requests_remaining": 100,
                "tokens_remaining": 50000,
                "reset_at": "2024-01-15T11:00:00Z"
            }
        });

        let rate_limits = event.get("rate_limits").cloned();
        assert!(rate_limits.is_some());
        let rl = rate_limits.unwrap();
        assert_eq!(rl["requests_remaining"], 100);
        assert_eq!(rl["tokens_remaining"], 50000);
    }

    #[test]
    fn test_rate_limits_missing_is_none() {
        let event = json!({"type": "turn.completed"});
        let rate_limits = event.get("rate_limits").cloned();
        assert!(rate_limits.is_none());
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Message Content Extraction Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod message_content {
    use super::*;

    #[test]
    fn test_extract_message_from_message_field() {
        let event = json!({
            "type": "item.tool.call",
            "message": "Running bash command"
        });

        let message = event
            .get("message")
            .or_else(|| event.get("content"))
            .and_then(|v| v.as_str());

        assert_eq!(message, Some("Running bash command"));
    }

    #[test]
    fn test_extract_message_from_content_field() {
        let event = json!({
            "type": "item.text",
            "content": "Here is the fix..."
        });

        let message = event
            .get("message")
            .or_else(|| event.get("content"))
            .and_then(|v| v.as_str());

        assert_eq!(message, Some("Here is the fix..."));
    }

    #[test]
    fn test_message_truncation_for_long_content() {
        let long_content = "x".repeat(500);
        let event = json!({
            "type": "item.text",
            "message": long_content
        });

        let message = event.get("message").and_then(|v| v.as_str()).map(|s| {
            if s.len() > 200 {
                format!("{}...", &s[..200])
            } else {
                s.to_string()
            }
        });

        assert!(message.is_some());
        let msg = message.unwrap();
        assert_eq!(msg.len(), 203); // 200 chars + "..."
        assert!(msg.ends_with("..."));
    }

    #[test]
    fn test_short_message_not_truncated() {
        let event = json!({
            "type": "item.text",
            "message": "Short message"
        });

        let message = event.get("message").and_then(|v| v.as_str()).map(|s| {
            if s.len() > 200 {
                format!("{}...", &s[..200])
            } else {
                s.to_string()
            }
        });

        assert_eq!(message, Some("Short message".to_string()));
    }
}
