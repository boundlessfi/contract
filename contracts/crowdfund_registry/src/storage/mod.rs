use soroban_sdk::{contracttype, Address, BytesN, String};

// Local copies of governance_voting types for cross-contract serialization.
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
pub enum VoteStatus {
    Pending,
    Active,
    Concluded,
    Cancelled,
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
#[derive(Clone, Debug)]
pub struct VotingSession {
    pub session_id: BytesN<32>,
    pub context: VoteContext,
    pub module_id: u64,
    pub created_at: u64,
    pub start_at: u64,
    pub end_at: u64,
    pub status: VoteStatus,
    pub threshold: Option<u32>,
    pub threshold_reached: bool,
    pub total_votes: u32,
    pub quorum: Option<u32>,
    pub weight_by_reputation: bool,
}

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CampaignStatus {
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
pub enum CrowdfundMilestoneStatus {
    Pending,
    Submitted,
    Approved,
    Released,
    Rejected,
    Disputed,
}

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisputeResolution {
    ApproveCreator,
    ApproveBacker,
}

/// Reason a community vote rejected a campaign.
/// Replaces the old free-form String so the indexer can distinguish cases
/// without parsing strings.
#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VoteRejectionReason {
    /// "Reject" option received majority of votes.
    RejectMajority,
    /// Voting period expired before the approval threshold was reached.
    ExpiredWithoutApproval,
}

/// On-chain milestone state.
/// Descriptions live in the backend database — only financial state is stored here.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Milestone {
    pub id: u32,
    pub pct: u32, // percentage of total (basis points: 10000 = 100%)
    pub status: CrowdfundMilestoneStatus,
    pub flagged_at: u64, // 0 = not flagged; otherwise timestamp when overdue was flagged
}

/// On-chain campaign state.
/// Metadata (title, description, team, etc.) lives in the backend database.
/// Only financial and access-control state is stored here.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Campaign {
    pub id: u64,
    pub owner: Address,
    pub status: CampaignStatus,
    pub funding_goal: i128,
    pub current_funding: i128,
    pub asset: Address,
    pub pool_id: BytesN<32>,
    pub deadline: u64,
    pub milestone_count: u32,
    pub min_pledge: i128,
    pub backer_count: u32, // informational: total unique backers who pledged
    pub vote_session_id: Option<BytesN<32>>,
}

#[contracttype]
#[derive(Clone)]
pub enum CrowdfundDataKey {
    Admin,
    CoreEscrow,
    ReputationRegistry,
    GovernanceVoting,
    CampaignCount,
    Campaign(u64),
    // Decomposed milestones: no Vec in Campaign struct
    CampaignMilestone(u64, u32), // campaign_id, milestone_index -> Milestone
    // Pledge tracking: amount stored per backer for dispute/refund verification
    Pledge(u64, Address), // campaign_id, backer -> net amount pledged
}
