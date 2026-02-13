use crate::storage::ModuleType;
use soroban_sdk::{contractevent, Address};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeRateSet {
    pub module: ModuleType,
    pub rate_bps: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositRouted {
    pub payer: Address,
    pub module: ModuleType,
    pub amount: i128,
    pub fee: i128,
}
