use crate::storage::GrantType;
use soroban_sdk::{contractevent, Address};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrantCreated {
    pub id: u64,
    pub grant_type: GrantType,
    pub creator: Address,
    pub budget: i128,
}
