use boundless_types::{ModuleType, SubType};
use soroban_sdk::{contractevent, Address, BytesN};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolCreated {
    #[topic]
    pub pool_id: BytesN<32>,
    pub owner: Address,
    pub module: ModuleType,
    pub total_amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolLocked {
    #[topic]
    pub pool_id: BytesN<32>,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotReleased {
    #[topic]
    pub pool_id: BytesN<32>,
    pub slot_index: u32,
    pub recipient: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refunded {
    #[topic]
    pub pool_id: BytesN<32>,
    pub recipient: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsuranceContributed {
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsuranceClaimed {
    pub claimant: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeCharged {
    #[topic]
    pub pool_id: BytesN<32>,
    pub sub_type: SubType,
    pub gross: i128,
    pub fee: i128,
    pub treasury_cut: i128,
    pub insurance_cut: i128,
    pub net: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeRateUpdated {
    pub old_bps: u32,
    pub new_bps: u32,
}
