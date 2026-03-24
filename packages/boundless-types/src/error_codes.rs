/// Reserved error code ranges per contract to avoid collision.
///
/// Each contract should define its own `#[contracterror]` enum
/// with values within the assigned range.
///
/// Ranges:
///   1-99:     Shared / generic
///   100-199:  CoreEscrow
///   200-299:  (reserved - was PaymentRouter, now merged into CoreEscrow)
///   300-399:  ReputationRegistry
///   400-499:  (reserved - was SparkCredits, now merged into ReputationRegistry)
///   500-599:  GovernanceVoting
///   600-699:  ProjectRegistry
///   700-799:  BountyRegistry
///   800-899:  CrowdfundRegistry
///   900-999:  GrantHub
///   1000-1099: HackathonRegistry

// Shared error codes
pub const ERR_NOT_INITIALIZED: u32 = 1;
pub const ERR_ALREADY_INITIALIZED: u32 = 2;
pub const ERR_UNAUTHORIZED: u32 = 3;
pub const ERR_NOT_FOUND: u32 = 4;
pub const ERR_INVALID_AMOUNT: u32 = 5;
pub const ERR_INVALID_STATE: u32 = 6;
pub const ERR_DEADLINE_PASSED: u32 = 7;
pub const ERR_DEADLINE_NOT_REACHED: u32 = 8;
pub const ERR_OVERFLOW: u32 = 9;
pub const ERR_PAUSED: u32 = 10;
