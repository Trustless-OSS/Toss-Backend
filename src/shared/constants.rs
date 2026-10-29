use rust_decimal::Decimal;

pub const TESTNET_USDC: &str = "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5";
pub const TRUSTLESS_WORK_FEE_BPS: i64 = 30;
pub const BASIS_POINTS: i64 = 10_000;

pub const TOSS_PLATFORM_FEE_BPS: i64 = 100;

pub fn toss_platform_fee_percent() -> Decimal {
    Decimal::from(TOSS_PLATFORM_FEE_BPS) * Decimal::from(100) / Decimal::from(BASIS_POINTS)
}

pub const ESCROW_INIT_MILESTONE_USDC: f64 = 0.01;

// BullMQ Worker Configuration Constants
// These values are tuned for typical usage and rarely need adjustment.
// Lock duration: how long a worker holds a job lock before re-running stall detection
pub const BULLMQ_LOCK_DURATION_MS: u64 = 30_000;
// Stall detection interval: how often workers scan for stalled jobs
pub const BULLMQ_STALLED_INTERVAL_MS: u64 = 30_000;
// Stall recoveries: allowed stall recoveries before the job is failed (strict: 1 = fail on first stall)
pub const BULLMQ_MAX_STALLED_COUNT: u32 = 1;

// Email Service Defaults
pub const DEFAULT_EMAIL_FROM: &str = "Trustless-OSS <info@trustless-oss.xyz>";

/// A GitHub label that TOSS creates for every connected repository.
#[derive(Debug, Clone, Copy)]
pub struct DefaultGithubLabel {
    pub name: &'static str,
    /// Six-character GitHub hex color without a leading `#`.
    pub color: &'static str,
    pub description: &'static str,
}

/// Labels required for bounty discovery and priority classification.
pub const DEFAULT_GITHUB_REPOSITORY_LABELS: [DefaultGithubLabel; 4] = [
    DefaultGithubLabel {
        name: "rewarded",
        color: "0E8A16",
        description: "Issue has a bounty",
    },
    DefaultGithubLabel {
        name: "low",
        color: "2DA44E",
        description: "Low priority",
    },
    DefaultGithubLabel {
        name: "medium",
        color: "FBCA04",
        description: "Medium priority",
    },
    DefaultGithubLabel {
        name: "high",
        color: "D93F0B",
        description: "High priority",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_fee_is_one_percent() {
        assert_eq!(toss_platform_fee_percent(), Decimal::ONE);
    }
}
