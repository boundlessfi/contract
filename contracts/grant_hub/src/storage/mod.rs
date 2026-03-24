use soroban_sdk::{contracttype, Address, BytesN, String};

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantType {
    Milestone,
    Retrospective,
    QF,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantStatus {
    Pending,
    Active,
    Executing,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MilestoneStatus {
    Pending,
    Submitted,
    Approved,
    Released,
    Rejected,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Grant {
    pub id: u64,
    pub creator: Address,
    pub grant_type: GrantType,
    pub status: GrantStatus,
    pub amount: i128,
    pub asset: Address,
    pub pool_id: BytesN<32>,
    pub milestone_count: u32,
    pub metadata_cid: String,
    pub created_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct GrantMilestone {
    pub id: u32,
    pub description: String,
    pub pct: u32, // basis points, all milestones sum to 10000
    pub status: MilestoneStatus,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct QFRoundData {
    pub session_id: BytesN<32>,
    pub matching_pool: i128,
    pub project_count: u32,
}

// Local copies of governance_voting types for cross-contract deserialization.
// Field names and order must match the canonical definitions in governance_voting.

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VoteContext {
    CampaignValidation,
    RetrospectiveGrant,
    QFRound,
    HackathonJudging,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct VoteOption {
    pub id: u32,
    pub label: String,
    pub votes: u32,
    pub weighted_votes: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    CoreEscrow,
    ReputationRegistry,
    GovernanceVoting,
    GrantCount,
    Grant(u64),
    GrantMilestone(u64, u32), // grant_id, milestone_index
    GrantRecipient(u64),      // grant_id -> Address (milestone grant recipient)
    QFRound(u64),             // grant_id -> QFRoundData
    RetroSession(u64),        // grant_id -> BytesN<32>
}
