use soroban_sdk::{contracttype, Address, BytesN, String, Vec};

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantType {
    Milestone,
    Retrospective,
    Quadratic,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantStatus {
    Draft,
    Active,
    Voting,
    Distributing,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MilestoneStatus {
    Pending,
    Submitted,
    Approved,
    Rejected,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct GrantMilestone {
    pub index: u32,
    pub description_cid: String,
    pub amount: i128,
    pub status: MilestoneStatus,
    pub submission_cid: Option<String>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Grant {
    pub id: u64,
    pub grant_type: GrantType,
    pub creator: Address,
    pub project_id: u64,
    pub metadata_cid: String,
    pub status: GrantStatus,
    pub total_budget: i128,
    pub asset: Address,
    pub pool_id: BytesN<32>,
    pub recipient: Option<Address>,
    pub milestones: Vec<GrantMilestone>,
    pub vote_session_id: Option<BytesN<32>>,
    pub applicants: Vec<Address>,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    ProjectRegistry,
    CoreEscrow,
    GovernanceVoting,
    ReputationRegistry,
    PaymentRouter,
    GrantCount,
    Grant(u64),
}
