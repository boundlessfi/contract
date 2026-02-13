use soroban_sdk::{contracttype, Address, String};

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub id: u64,
    pub owner: Address,
    pub org_name: String,
    pub metadata_cid: String,
    pub verification_level: u32,
    pub deposit_held: i128,
    pub active_bounty_budget: i128,
    pub total_bounties_posted: u32,
    pub total_paid_out: i128,
    pub avg_contributor_rating: u32,
    pub dispute_count: u32,
    pub missed_milestones: u32,
    pub warning_level: u32,
    pub suspended: bool,
    pub hackathons_hosted: u32,
    pub grants_distributed: i128,
    pub campaigns_launched: u32,
    pub total_platform_spend: i128,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    TokenAsset,
    ProjectCount,
    Project(u64),
    CoreEscrow,
    AuthorizedModule(Address),
}
