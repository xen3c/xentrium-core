/// Protocol constants

/// Address prefix
pub const ADDRESS_PREFIX: &str = "XNT";

/// Internal precision: 8 decimals
/// 1 XNT = 100,000,000 units
pub const INTERNAL_PRECISION: u64 = 100_000_000;

/// Minimum unit: 0.00000001 XNT = 1 unit
pub const MIN_UNIT: u64 = 1;

/// Default fee: 0.0001 XNT = 10,000 units
pub const DEFAULT_FEE: u64 = 10_000;

/// Fee floor: 0.00000001 XNT = 1 unit
pub const FEE_FLOOR: u64 = 1;

/// Maximum supply: 100,000,000 XNT
pub const MAX_SUPPLY: u64 = 100_000_000 * INTERNAL_PRECISION;

/// Genesis founder wallet: 5,000,000 XNT
pub const GENESIS_FOUNDER_WALLET: u64 = 5_000_000 * INTERNAL_PRECISION;

/// Reward pool: 95,000,000 XNT
pub const REWARD_POOL: u64 = 95_000_000 * INTERNAL_PRECISION;

/// Hardcoded emission: 81,000,000 XNT
pub const HARDCODED_EMISSION: u64 = 81_000_000 * INTERNAL_PRECISION;

/// Reserve: 14,000,000 XNT
pub const RESERVE: u64 = 14_000_000 * INTERNAL_PRECISION;

/// Validator minimum after day 365: 100 XNT
pub const VALIDATOR_MINIMUM_AFTER_365: u64 = 100 * INTERNAL_PRECISION;

/// Committee activation threshold: 21 validators
pub const CONSENSUS_ACTIVATION_THRESHOLD: usize = 21;

/// Max committee size: 51 validators
pub const MAX_COMMITTEE_SIZE: usize = 51;

/// Max candidate pool: 500 validators
pub const MAX_CANDIDATE_POOL: usize = 500;

/// Consensus threshold: 67%
pub const CONSENSUS_THRESHOLD: f64 = 0.67;

/// Committee weight cap: 5%
pub const COMMITTEE_WEIGHT_CAP: f64 = 0.05;

/// Auto-lock timeout: 10 minutes
pub const AUTO_LOCK_MINUTES: u64 = 10;

/// Validator pending period: 7 days
pub const VALIDATOR_PENDING_DAYS: u64 = 7;

/// Transaction retention: 30 days
pub const TRANSACTION_RETENTION_DAYS: u64 = 30;

/// Snapshot frequency: 24 hours
pub const SNAPSHOT_INTERVAL_HOURS: u64 = 24;

/// Bootstrap exception expires at: 500 validators OR 365 days
pub const BOOTSTRAP_VALIDATOR_LIMIT: usize = 500;
pub const BOOTSTRAP_EXPIRY_DAYS: u64 = 365;
