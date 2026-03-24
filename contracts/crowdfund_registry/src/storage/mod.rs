use soroban_sdk::{contracttype, Address, BytesN, String, Vec};

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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CampaignStatus {
    Draft,
    Submitted,
    Validated,
    Campaigning,
    Funded,
    Executing,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MilestoneStatus {
    Pending,
    Submitted,
    Approved,
    Released,
    Rejected,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Milestone {
    pub id: u32,
    pub description: String,
    pub amount: i128,
    pub status: MilestoneStatus,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Campaign {
    pub id: u64,
    pub owner: Address,
    pub project_id: u64,
    pub metadata_cid: String,
    pub status: CampaignStatus,
    pub funding_goal: i128,
    pub current_funding: i128,
    pub asset: Address,
    pub pool_id: BytesN<32>,
    pub deadline: u64,
    pub milestones: Vec<Milestone>,
    pub min_pledge: i128,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    ProjectRegistry,
    CoreEscrow,
    VotingContract,
    ReputationRegistry,
    PaymentRouter,
    FeeAccount,
    Treasury,
    CampaignCount,
    Campaign(u64),
    Pledge(u64, Address), // campaign_id, contributor -> amount
    Donors(u64),          // campaign_id -> Vec<Address>
}
