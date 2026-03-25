use soroban_sdk::{contracttype, Address, BytesN, String};

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
#[derive(Clone, Debug)]
pub struct VoteRecord {
    pub voter: Address,
    pub option_id: u32,
    pub weight: u32,
    pub voted_at: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct QFDonation {
    pub donor: Address,
    pub option_id: u32,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone)]
pub enum GovernanceDataKey {
    Admin,
    Version,
    AuthorizedModule(Address),
    Session(BytesN<32>),
    VoteOption(BytesN<32>, u32),
    VoteRecord(BytesN<32>, Address),
    QFDonation(BytesN<32>, Address, u32),
    OptionCount(BytesN<32>),
    // Stores the scaled sum of sqrt(donation) for each option (for QF)
    OptionSumSqrt(BytesN<32>, u32),
}
