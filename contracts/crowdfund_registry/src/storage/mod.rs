use soroban_sdk::{contracttype, Address, BytesN, String};

// Local copy of governance_voting VoteContext for cross-contract serialization.
#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VoteContext {
    CampaignValidation,
    RetrospectiveGrant,
    QFRound,
    HackathonJudging,
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
    Failed,
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
    Disputed,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Milestone {
    pub id: u32,
    pub description: String,
    pub pct: u32, // percentage of total (basis points: 10000 = 100%)
    pub status: MilestoneStatus,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Campaign {
    pub id: u64,
    pub owner: Address,
    pub metadata_cid: String,
    pub status: CampaignStatus,
    pub funding_goal: i128,
    pub current_funding: i128,
    pub asset: Address,
    pub pool_id: BytesN<32>,
    pub deadline: u64,
    pub milestone_count: u32,
    pub min_pledge: i128,
    pub backer_count: u32,
    pub refund_progress: u32,
    pub vote_session_id: Option<BytesN<32>>,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    CoreEscrow,
    ReputationRegistry,
    GovernanceVoting,
    CampaignCount,
    Campaign(u64),
    // Decomposed milestones: no Vec in Campaign struct
    CampaignMilestone(u64, u32), // campaign_id, milestone_index -> Milestone
    // Pledge tracking
    Pledge(u64, Address), // campaign_id, backer -> amount
    // Backer list stored in batches of 50
    BackerBatch(u64, u32), // campaign_id, batch_index -> Vec<Address> (max 50)
}
