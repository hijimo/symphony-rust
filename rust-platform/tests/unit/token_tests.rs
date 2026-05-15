//! Unit tests for token accounting logic.
//!
//! Tests cover:
//! - Delta calculation from absolute values
//! - Regression detection (absolute value goes backwards)
//! - Cumulative totals correctness
//! - Runtime seconds accumulation

use std::time::{Duration, Instant};

// ═══════════════════════════════════════════════════════════════════════════════
// Token Accounting Implementation (for testing)
// ═══════════════════════════════════════════════════════════════════════════════

/// Token accounting state for a single agent session.
///
/// The Codex app-server reports ABSOLUTE token counts. We compute deltas
/// by comparing against the last reported values.
#[derive(Debug, Clone)]
struct TokenAccounting {
    /// Last reported absolute input tokens
    last_reported_input: u64,
    /// Last reported absolute output tokens
    last_reported_output: u64,
    /// Last reported absolute total tokens
    last_reported_total: u64,
    /// Cumulative input tokens (sum of all deltas)
    cumulative_input: u64,
    /// Cumulative output tokens (sum of all deltas)
    cumulative_output: u64,
    /// Cumulative total tokens
    cumulative_total: u64,
    /// Session start time (for runtime calculation)
    started_at: Instant,
    /// Number of updates received
    update_count: u32,
    /// Whether a regression was detected
    regression_detected: bool,
}

impl TokenAccounting {
    fn new() -> Self {
        Self {
            last_reported_input: 0,
            last_reported_output: 0,
            last_reported_total: 0,
            cumulative_input: 0,
            cumulative_output: 0,
            cumulative_total: 0,
            started_at: Instant::now(),
            update_count: 0,
            regression_detected: false,
        }
    }

    /// Update with new absolute token values from the agent.
    ///
    /// Computes deltas and adds to cumulative totals.
    /// If absolute values go backwards (regression), logs a warning
    /// and uses the new values as the new baseline without adding negative deltas.
    fn update(&mut self, input_tokens: u64, output_tokens: u64, total_tokens: u64) {
        self.update_count += 1;

        // Compute deltas
        let input_delta = if input_tokens >= self.last_reported_input {
            input_tokens - self.last_reported_input
        } else {
            // Regression detected — absolute value went backwards
            self.regression_detected = true;
            0
        };

        let output_delta = if output_tokens >= self.last_reported_output {
            output_tokens - self.last_reported_output
        } else {
            self.regression_detected = true;
            0
        };

        let total_delta = if total_tokens >= self.last_reported_total {
            total_tokens - self.last_reported_total
        } else {
            self.regression_detected = true;
            0
        };

        // Accumulate deltas
        self.cumulative_input += input_delta;
        self.cumulative_output += output_delta;
        self.cumulative_total += total_delta;

        // Update last reported values (always update, even on regression)
        self.last_reported_input = input_tokens;
        self.last_reported_output = output_tokens;
        self.last_reported_total = total_tokens;
    }

    /// Get the runtime duration since session start.
    fn runtime_seconds(&self) -> f64 {
        self.started_at.elapsed().as_secs_f64()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

mod delta_calculation {
    use super::*;

    #[test]
    fn test_first_update_is_full_delta() {
        let mut accounting = TokenAccounting::new();
        accounting.update(1000, 500, 1500);

        assert_eq!(accounting.cumulative_input, 1000);
        assert_eq!(accounting.cumulative_output, 500);
        assert_eq!(accounting.cumulative_total, 1500);
    }

    #[test]
    fn test_subsequent_updates_compute_delta() {
        let mut accounting = TokenAccounting::new();

        // First update: absolute 1000/500/1500
        accounting.update(1000, 500, 1500);
        assert_eq!(accounting.cumulative_input, 1000);
        assert_eq!(accounting.cumulative_output, 500);

        // Second update: absolute 2500/1200/3700
        // Delta: 1500/700/2200
        accounting.update(2500, 1200, 3700);
        assert_eq!(accounting.cumulative_input, 2500);
        assert_eq!(accounting.cumulative_output, 1200);
        assert_eq!(accounting.cumulative_total, 3700);
    }

    #[test]
    fn test_zero_delta_when_no_change() {
        let mut accounting = TokenAccounting::new();

        accounting.update(1000, 500, 1500);
        accounting.update(1000, 500, 1500); // Same values

        assert_eq!(accounting.cumulative_input, 1000);
        assert_eq!(accounting.cumulative_output, 500);
        assert_eq!(accounting.cumulative_total, 1500);
        assert_eq!(accounting.update_count, 2);
    }

    #[test]
    fn test_multiple_incremental_updates() {
        let mut accounting = TokenAccounting::new();

        accounting.update(100, 50, 150);
        accounting.update(300, 150, 450);
        accounting.update(600, 300, 900);
        accounting.update(1000, 500, 1500);

        // Cumulative should equal the final absolute values
        assert_eq!(accounting.cumulative_input, 1000);
        assert_eq!(accounting.cumulative_output, 500);
        assert_eq!(accounting.cumulative_total, 1500);
        assert_eq!(accounting.update_count, 4);
    }
}

mod regression_detection {
    use super::*;

    #[test]
    fn test_regression_detected_when_input_decreases() {
        let mut accounting = TokenAccounting::new();

        accounting.update(1000, 500, 1500);
        assert!(!accounting.regression_detected);

        // Input goes backwards
        accounting.update(800, 600, 1400);
        assert!(accounting.regression_detected);

        // Cumulative input should NOT decrease
        assert_eq!(accounting.cumulative_input, 1000); // No negative delta added
        assert_eq!(accounting.cumulative_output, 600); // Output increased normally
    }

    #[test]
    fn test_regression_detected_when_output_decreases() {
        let mut accounting = TokenAccounting::new();

        accounting.update(1000, 500, 1500);
        // Output goes backwards
        accounting.update(1200, 400, 1600);

        assert!(accounting.regression_detected);
        assert_eq!(accounting.cumulative_output, 500); // No negative delta
        assert_eq!(accounting.cumulative_input, 1200); // Input increased normally
    }

    #[test]
    fn test_regression_resets_baseline() {
        let mut accounting = TokenAccounting::new();

        accounting.update(1000, 500, 1500);
        // Regression
        accounting.update(800, 400, 1200);
        // Now increase from the new baseline
        accounting.update(1500, 900, 2400);

        // Delta from 800→1500 = 700 for input
        // Cumulative: 1000 (first) + 0 (regression) + 700 (recovery) = 1700
        assert_eq!(accounting.cumulative_input, 1700);
        // Delta from 400→900 = 500 for output
        // Cumulative: 500 (first) + 0 (regression) + 500 (recovery) = 1000
        assert_eq!(accounting.cumulative_output, 1000);
    }

    #[test]
    fn test_no_regression_on_normal_increase() {
        let mut accounting = TokenAccounting::new();

        accounting.update(100, 50, 150);
        accounting.update(200, 100, 300);
        accounting.update(300, 150, 450);

        assert!(!accounting.regression_detected);
    }
}

mod cumulative_totals {
    use super::*;

    #[test]
    fn test_cumulative_matches_final_absolute_on_monotonic_increase() {
        let mut accounting = TokenAccounting::new();

        accounting.update(500, 200, 700);
        accounting.update(1500, 800, 2300);
        accounting.update(3000, 1500, 4500);

        // For monotonically increasing values, cumulative == final absolute
        assert_eq!(accounting.cumulative_input, 3000);
        assert_eq!(accounting.cumulative_output, 1500);
        assert_eq!(accounting.cumulative_total, 4500);
    }

    #[test]
    fn test_cumulative_across_multiple_sessions() {
        // Simulate aggregating across multiple sessions
        let mut session1 = TokenAccounting::new();
        session1.update(1000, 500, 1500);

        let mut session2 = TokenAccounting::new();
        session2.update(2000, 1000, 3000);

        let total_input = session1.cumulative_input + session2.cumulative_input;
        let total_output = session1.cumulative_output + session2.cumulative_output;

        assert_eq!(total_input, 3000);
        assert_eq!(total_output, 1500);
    }

    #[test]
    fn test_zero_tokens_initial_state() {
        let accounting = TokenAccounting::new();

        assert_eq!(accounting.cumulative_input, 0);
        assert_eq!(accounting.cumulative_output, 0);
        assert_eq!(accounting.cumulative_total, 0);
        assert_eq!(accounting.update_count, 0);
    }
}

mod runtime_seconds {
    use super::*;
    use std::thread;

    #[test]
    fn test_runtime_increases_over_time() {
        let accounting = TokenAccounting::new();

        // Small sleep to ensure measurable time passes
        thread::sleep(Duration::from_millis(10));

        let runtime = accounting.runtime_seconds();
        assert!(runtime >= 0.01); // At least 10ms
    }

    #[test]
    fn test_runtime_starts_at_zero() {
        let accounting = TokenAccounting::new();
        let runtime = accounting.runtime_seconds();
        // Should be very close to zero (within 100ms of creation)
        assert!(runtime < 0.1);
    }
}
