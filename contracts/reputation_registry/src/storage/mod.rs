use soroban_sdk::{contracttype, Address, Map, String};

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActivityCategory {
    Development,
    Design,
    Marketing,
    Security,
    Community,
}

#[contracttype]
#[derive(Clone)]
pub struct ContributorProfile {
    pub address: Address,
    pub overall_score: u32,
    pub level: u32,
    pub category_scores: Map<ActivityCategory, u32>,
    pub bounties_completed: u32,
    pub hackathons_entered: u32,
    pub hackathons_won: u32,
    pub total_earned: i128,
    pub metadata_cid: String,
    pub joined_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Profile(Address),
    AuthorizedModule(Address), // address -> bool
}
