use soroban_sdk::{contracttype, Address, BytesN};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    EscrowPool(BytesN<32>),
    ReleaseSlot(BytesN<32>, u32), // (pool_id, index)
    InsuranceFund,
    Admin,
    FeeAccount,
    Treasury,
}

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleType {
    Bounty,
    Crowdfund,
    Grant,
    Hackathon,
}

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

#[contracttype]
#[derive(Clone)]
pub struct InsuranceFund {
    pub balance: i128,
    pub total_contributions: i128,
    pub total_paid_out: i128,
}
