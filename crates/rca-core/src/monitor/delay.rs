//! Answer delay strategy using Poisson distribution.
//!
//! Ported from the Python version's `calculate_waittime` in `Utils.py`.
//! The idea is to produce a random delay that makes the answer submission
//! timing appear natural, using a Poisson distribution parameterized by
//! the problem's time limit and the configured strategy.

use std::time::Duration;

use rand::Rng;

/// Strategy for how long to wait before submitting an auto-answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DelayStrategy {
    /// Moderate: target ~65% of the time limit.
    #[default]
    Moderate,
    /// Aggressive: target ~35% of the time limit.
    Aggressive,
    /// Conservative: target ~85% of the time limit.
    Conservative,
    /// Custom: user-specified target percentage (0-100).
    Custom { percent: u32 },
}

impl DelayStrategy {
    /// Map strategy to target percentage (0.0–1.0) of the time limit.
    fn target_fraction(self) -> f64 {
        match self {
            Self::Moderate => 0.65,
            Self::Aggressive => 0.35,
            Self::Conservative => 0.85,
            Self::Custom { percent } => (percent.min(100) as f64) / 100.0,
        }
    }

    /// Create a `DelayStrategy` from a numeric type code.
    /// 1 = Moderate, 2 = Aggressive, 3 = Conservative, 4+ = Custom.
    pub fn from_type_code(code: u32, custom_percent: u32) -> Self {
        match code {
            1 => Self::Moderate,
            2 => Self::Aggressive,
            3 => Self::Conservative,
            _ => Self::Custom {
                percent: custom_percent.min(100),
            },
        }
    }

    /// Convert back to a numeric type code.
    pub fn to_type_code(self) -> u32 {
        match self {
            Self::Moderate => 1,
            Self::Aggressive => 2,
            Self::Conservative => 3,
            Self::Custom { .. } => 4,
        }
    }
}

/// Calculate a random wait time before submitting an answer.
///
/// Uses a Poisson distribution centered around `target_fraction * limit_secs`.
/// If the remaining time is too short (< 15 seconds), returns Duration::ZERO
/// for immediate submission.
///
/// # Arguments
/// * `limit_secs` - The problem's time limit in seconds. If `None`, defaults to 60s.
/// * `strategy` - The delay strategy to use.
///
/// # Returns
/// The duration to wait before submitting the answer.
pub fn calculate_wait_time(limit_secs: Option<i64>, strategy: &DelayStrategy) -> Duration {
    let limit = limit_secs.unwrap_or(60).max(0) as f64;

    // If time limit is too short, submit immediately
    if limit < 15.0 {
        return Duration::ZERO;
    }

    let target = limit * strategy.target_fraction();
    let lambda = compute_lambda(limit, target);
    let sample = poisson_sample(lambda);

    // Clamp to [1, limit - 5] to avoid timing out but never be instant
    let wait = sample.clamp(1.0, (limit - 5.0).max(1.0));
    Duration::from_secs_f64(wait)
}

/// Compute the Poisson lambda parameter such that the expected value
/// is approximately `target` seconds.
fn compute_lambda(limit: f64, target: f64) -> f64 {
    // Lambda = target so the expected wait time ≈ target
    // Clamped to reasonable bounds to avoid degenerate distributions
    target.clamp(1.0, limit * 0.95)
}

/// Generate a single sample from Poisson(lambda) using the inverse transform method.
///
/// Returns a value in seconds that is approximately Poisson-distributed.
fn poisson_sample(lambda: f64) -> f64 {
    let mut rng = rand::rng();
    let l = (-lambda).exp();
    let mut k = 0u32;
    let mut p = 1.0_f64;

    loop {
        k += 1;
        p *= rng.random::<f64>();
        if p <= l {
            break;
        }
    }

    (k.saturating_sub(1)) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moderate_strategy_produces_reasonable_delay() {
        // With a 60s limit, moderate (65%) should produce delays around 39s
        let mut total = 0.0;
        let n = 1000;
        for _ in 0..n {
            let d = calculate_wait_time(Some(60), &DelayStrategy::Moderate);
            let secs = d.as_secs_f64();
            assert!(secs >= 1.0, "delay too short: {secs}");
            assert!(secs <= 55.0, "delay too long: {secs}");
            total += secs;
        }
        let avg = total / n as f64;
        // Average should be roughly around 39s (65% of 60)
        assert!(
            (20.0..=55.0).contains(&avg),
            "average delay {avg} out of expected range"
        );
    }

    #[test]
    fn aggressive_strategy_produces_shorter_delay() {
        let mut total = 0.0;
        let n = 1000;
        for _ in 0..n {
            let d = calculate_wait_time(Some(60), &DelayStrategy::Aggressive);
            total += d.as_secs_f64();
        }
        let avg = total / n as f64;
        // Aggressive (35%) of 60 = ~21s
        assert!(
            (10.0..=40.0).contains(&avg),
            "average delay {avg} out of expected range for aggressive"
        );
    }

    #[test]
    fn conservative_strategy_produces_longer_delay() {
        let mut total = 0.0;
        let n = 1000;
        for _ in 0..n {
            let d = calculate_wait_time(Some(60), &DelayStrategy::Conservative);
            total += d.as_secs_f64();
        }
        let avg = total / n as f64;
        // Conservative (85%) of 60 = ~51s
        assert!(
            (35.0..=55.0).contains(&avg),
            "average delay {avg} out of expected range for conservative"
        );
    }

    #[test]
    fn short_limit_returns_zero() {
        let d = calculate_wait_time(Some(10), &DelayStrategy::Moderate);
        assert_eq!(d, Duration::ZERO);
    }

    #[test]
    fn none_limit_uses_default_60s() {
        let d = calculate_wait_time(None, &DelayStrategy::Moderate);
        let secs = d.as_secs_f64();
        assert!((1.0..=55.0).contains(&secs));
    }

    #[test]
    fn custom_percent_works() {
        let strategy = DelayStrategy::Custom { percent: 50 };
        let d = calculate_wait_time(Some(60), &strategy);
        let secs = d.as_secs_f64();
        assert!((1.0..=55.0).contains(&secs));
    }

    #[test]
    fn from_type_code_roundtrip() {
        assert_eq!(DelayStrategy::from_type_code(1, 0), DelayStrategy::Moderate);
        assert_eq!(
            DelayStrategy::from_type_code(2, 0),
            DelayStrategy::Aggressive
        );
        assert_eq!(
            DelayStrategy::from_type_code(3, 0),
            DelayStrategy::Conservative
        );
        assert_eq!(
            DelayStrategy::from_type_code(4, 50),
            DelayStrategy::Custom { percent: 50 }
        );
    }

    #[test]
    fn to_type_code_roundtrip() {
        assert_eq!(DelayStrategy::Moderate.to_type_code(), 1);
        assert_eq!(DelayStrategy::Aggressive.to_type_code(), 2);
        assert_eq!(DelayStrategy::Conservative.to_type_code(), 3);
        assert_eq!(DelayStrategy::Custom { percent: 42 }.to_type_code(), 4);
    }

    #[test]
    fn custom_percent_zero_produces_instant() {
        // 0% target → lambda ≈ 0, sample ≈ 0, but clamped to 1.0
        let d = calculate_wait_time(Some(60), &DelayStrategy::Custom { percent: 0 });
        let secs = d.as_secs_f64();
        assert!((1.0..=55.0).contains(&secs));
    }

    #[test]
    fn custom_percent_100_produces_long_delay() {
        let mut total = 0.0;
        let n = 500;
        for _ in 0..n {
            let d = calculate_wait_time(Some(60), &DelayStrategy::Custom { percent: 100 });
            total += d.as_secs_f64();
        }
        let avg = total / n as f64;
        // 100% of 60 → target 57 (clamped to 95%), should produce long delays
        assert!(avg > 30.0, "average delay {avg} too short for 100% target");
    }

    #[test]
    fn custom_percent_over_100_is_clamped() {
        let strategy = DelayStrategy::Custom { percent: 150 };
        // Should behave the same as percent: 100
        let d = calculate_wait_time(Some(60), &strategy);
        let secs = d.as_secs_f64();
        assert!((1.0..=55.0).contains(&secs));
    }

    #[test]
    fn custom_from_type_code_over_100_is_clamped() {
        let strategy = DelayStrategy::from_type_code(4, 200);
        assert_eq!(strategy, DelayStrategy::Custom { percent: 100 });
    }

    #[test]
    fn negative_limit_treated_as_zero() {
        let d = calculate_wait_time(Some(-10), &DelayStrategy::Moderate);
        assert_eq!(d, Duration::ZERO);
    }

    #[test]
    fn default_strategy_is_moderate() {
        assert_eq!(DelayStrategy::default(), DelayStrategy::Moderate);
    }
}
