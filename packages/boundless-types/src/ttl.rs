/// TTL constants and helpers for Soroban storage tiers.
///
/// | Tier       | Used For                                    | TTL          |
/// |------------|---------------------------------------------|--------------|
/// | Instance   | Admin, fee configs, contract addrs, counters | 120 days     |
/// | Persistent | Bounties, campaigns, profiles, pools        | 600 days     |
/// | Temporary  | Refund progress, session cache              | 7 days       |

// Ledger close cadence: ~5 seconds → ~17_280 ledgers/day

/// Instance storage: extend when remaining TTL drops below this
pub const INSTANCE_TTL_THRESHOLD: u32 = 120 * 17_280; // ~120 days
/// Instance storage: extend to this TTL
pub const INSTANCE_TTL_EXTEND: u32 = 120 * 17_280; // ~120 days

/// Persistent storage: extend when remaining TTL drops below this
pub const PERSISTENT_TTL_THRESHOLD: u32 = 120 * 17_280; // ~120 days
/// Persistent storage: extend to this TTL
pub const PERSISTENT_TTL_EXTEND: u32 = 600 * 17_280; // ~600 days

/// Temporary storage: extend to this TTL
pub const TEMPORARY_TTL_EXTEND: u32 = 7 * 17_280; // ~7 days
