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
    pub weighted_votes: i128,
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
#[derive(Clone)]
pub struct VoteRecord {
    pub voter: Address,
    pub option_id: u32,
    pub weight: u32,
    pub timestamp: u64,
}

// For QF: tracking donations per project
#[contracttype]
#[derive(Clone)]
pub struct QFDonation {
    pub donor: Address,
    pub option_id: u32, // project index in session
    pub amount: i128,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    ReputationRegistry,
    Session(BytesN<32>),
    Option(BytesN<32>, u32),        // session_id, option_id -> VoteOption
    OptionCount(BytesN<32>),        // session_id -> u32
    Vote(BytesN<32>, Address),      // (session_id, voter) -> VoteRecord
    OptionSumSqrt(BytesN<32>, u32), // (session_id, option_id) -> i128 (sum of sqrt donations)
    AuthorizedModule(Address),      // address -> bool
}
