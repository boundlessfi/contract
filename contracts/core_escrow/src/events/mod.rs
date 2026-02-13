use crate::storage::ModuleType;
use soroban_sdk::{contractevent, Address, BytesN};

#[contractevent(topics = ["PoolCreated"], data_format = "vec")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolCreated {
    #[topic]
    pub pool_id: BytesN<32>,
    pub owner: Address,
    pub module: ModuleType,
    pub total_amount: i128,
}

#[contractevent(topics = ["PoolLocked"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolLocked {
    #[topic]
    pub pool_id: BytesN<32>,
}

#[contractevent(topics = ["SlotReleased"], data_format = "vec")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotReleased {
    #[topic]
    pub pool_id: BytesN<32>,
    pub slot_index: u32,
    pub recipient: Address,
    pub amount: i128,
}

#[contractevent(topics = ["Refunded"], data_format = "vec")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refunded {
    #[topic]
    pub pool_id: BytesN<32>,
    pub recipient: Address,
    pub amount: i128,
}

#[contractevent(topics = ["InsuranceContributed"], data_format = "single-value")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsuranceContributed {
    #[topic]
    pub asset: Address,
    pub amount: i128,
}

#[contractevent(topics = ["InsuranceClaimed"], data_format = "vec")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InsuranceClaimed {
    #[topic]
    pub asset: Address,
    pub claimant: Address,
    pub amount: i128,
}
