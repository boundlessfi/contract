use boundless_types::ActivityCategory;
use soroban_sdk::{contracttype, Address, Map, String};

#[contracttype]
#[derive(Clone)]
pub enum ReputationDataKey {
    Admin,
    Version,
    Profile(Address),
    CreditData(Address),
    AuthorizedModule(Address),
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
    pub campaigns_backed: u32,
    pub grants_received: u32,
    pub total_earned: i128,
    pub metadata_cid: String,
    pub joined_at: u64,
}

/// SparkCredits data (merged from SparkCredits contract)
#[contracttype]
#[derive(Clone)]
pub struct CreditData {
    pub credits: u32,
    pub max_credits: u32,
    pub last_recharge: u64,
    pub total_earned: u32,
    pub total_spent: u32,
}

impl CreditData {
    pub fn new(timestamp: u64) -> Self {
        CreditData {
            credits: 3, // starting credits
            max_credits: 10,
            last_recharge: timestamp,
            total_earned: 3,
            total_spent: 0,
        }
    }
}
