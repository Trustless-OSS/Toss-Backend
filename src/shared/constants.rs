pub const TESTNET_USDC: &str = "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5";
pub const TRUSTLESS_WORK_FEE_BPS: i64 = 30;
pub const BASIS_POINTS: i64 = 10_000;
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
pub const DEFAULT_EMAIL_FROM: &str = "TOSS <no-reply@trustless-oss.xyz>";
