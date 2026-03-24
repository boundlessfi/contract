use soroban_sdk::{contracttype, Address, BytesN};

use boundless_types::ModuleType;
use boundless_types::SubType;

// ── Storage Keys ──────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Treasury,
    FeeConfig,
    InsuranceFund,
    RoutingPaused,
    Version,
    // Pool data
    EscrowPool(BytesN<32>),
    ReleaseSlot(BytesN<32>, u32), // (pool_id, slot_index)
    SlotCount(BytesN<32>),
    // Fee audit trail
    FeeRecord(BytesN<32>), // per pool_id
    // Module authorization
    AuthorizedModule(Address),
}

// ── Escrow Pool ───────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub struct EscrowPool {
    pub pool_id: BytesN<32>,
    pub module: ModuleType,
    pub authorized_caller: Address,
    pub owner: Address,
    pub total_deposited: i128,
    pub total_released: i128,
    pub total_refunded: i128,
    pub asset: Address,
    pub locked: bool,
    pub created_at: u64,
    pub expires_at: u64,
}

// ── Release Slots ─────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub struct ReleaseSlot {
    pub pool_id: BytesN<32>,
    pub slot_index: u32,
    pub amount: i128,
    pub recipient: Address,
    pub released: bool,
    pub released_at: Option<u64>,
}

// ── Insurance Fund ────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub struct InsuranceFund {
    pub balance: i128,
    pub total_contributions: i128,
    pub total_paid_out: i128,
}

// ── Fee Configuration (merged from PaymentRouter) ─────────────────────────────

#[contracttype]
#[derive(Clone)]
pub struct FeeConfig {
    pub bounty_fee_bps: u32,
    pub crowdfund_fee_bps: u32,
    pub grant_fee_bps: u32,
    pub hackathon_fee_bps: u32,
    pub insurance_cut_bps: u32, // % of fee that goes to insurance (not of gross)
}

impl FeeConfig {
    pub fn default_config() -> Self {
        FeeConfig {
            bounty_fee_bps: 500,     // 5%
            crowdfund_fee_bps: 500,  // 5%
            grant_fee_bps: 300,      // 3%
            hackathon_fee_bps: 400,  // 4%
            insurance_cut_bps: 1000, // 10% of fee
        }
    }

    pub fn get_fee_bps(&self, sub_type: &SubType) -> u32 {
        match sub_type {
            SubType::BountyFCFS
            | SubType::BountyApplication
            | SubType::BountyContest
            | SubType::BountySplit => self.bounty_fee_bps,
            SubType::CrowdfundPledge => self.crowdfund_fee_bps,
            SubType::GrantMilestone
            | SubType::GrantRetrospective
            | SubType::GrantQFMatchingPool => self.grant_fee_bps,
            SubType::HackathonMain | SubType::HackathonTrack => self.hackathon_fee_bps,
        }
    }
}

// ── Fee Audit Record ──────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub struct FeeRecord {
    pub pool_id: BytesN<32>,
    pub sub_type: SubType,
    pub gross_amount: i128,
    pub fee_amount: i128,
    pub treasury_cut: i128,
    pub insurance_cut: i128,
    pub net_to_escrow: i128,
    pub timestamp: u64,
    pub payer: Address,
}
