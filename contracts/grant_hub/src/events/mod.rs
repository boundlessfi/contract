use crate::storage::GrantType;
use soroban_sdk::{contractevent, Address};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrantCreated {
    pub id: u64,
    pub grant_type: GrantType,
    pub creator: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneSubmitted {
    pub grant_id: u64,
    pub milestone_index: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneApproved {
    pub grant_id: u64,
    pub milestone_index: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrantCompleted {
    pub grant_id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QFDonationMade {
    pub grant_id: u64,
    pub project_index: u32,
    pub amount: i128,
}
