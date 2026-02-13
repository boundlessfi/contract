use crate::storage::VoteContext;
use soroban_sdk::{contractevent, Address, BytesN};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionCreated {
    pub session_id: BytesN<32>,
    pub context: VoteContext,
    pub module_id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoteCast {
    pub session_id: BytesN<32>,
    pub voter: Address,
    pub option_id: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QFDonationRecorded {
    pub session_id: BytesN<32>,
    pub donor: Address,
    pub option_id: u32,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleAuthorized {
    pub module: Address,
    pub authorized: bool,
}
