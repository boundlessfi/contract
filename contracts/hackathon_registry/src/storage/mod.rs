use soroban_sdk::{contracttype, Address, BytesN, String};

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HackathonStatus {
    Registration,
    Submission,
    Judging,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hackathon {
    pub id: u64,
    pub creator: Address,
    pub title: String,
    pub metadata_cid: String,
    pub status: HackathonStatus,
    pub prize_pool: i128,
    pub asset: Address,
    pub pool_id: BytesN<32>,
    pub registration_deadline: u64,
    pub submission_deadline: u64,
    pub judging_deadline: u64,
    pub judge_count: u32,
    pub submission_count: u32,
    pub max_participants: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submission {
    pub team_lead: Address,
    pub metadata_cid: String,
    pub submitted_at: u64,
    pub total_score: u32,
    pub score_count: u32,
    pub disqualified: bool,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SponsoredTrack {
    pub track_id: u32,
    pub hackathon_id: u64,
    pub sponsor: Address,
    pub track_name: String,
    pub prize_amount: i128,
    pub asset: Address,
    pub pool_id: BytesN<32>,
}

/// Per-winner record created at finalize time. Winners pull their prize via
/// `claim_prize`. After `claim_deadline` passes, the creator may reclaim
/// unclaimed amounts via `reclaim_unclaimed_prizes`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WinnerRecord {
    pub hackathon_id: u64,
    pub rank: u32,
    pub winner: Address,
    pub amount: i128,
    pub claimed: bool,
    pub claimed_at: Option<u64>,
    pub claim_deadline: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum HackathonDataKey {
    Admin,
    CoreEscrow,
    ReputationRegistry,
    HackathonCount,
    Hackathon(u64),
    Judge(u64, Address),
    JudgeIndex(u64, u32),
    Submission(u64, Address),
    SubmissionIndex(u64, u32),
    JudgeScore(u64, Address, Address),
    PrizeTier(u64, u32),
    HackathonTrack(u64, u32),
    HackathonTrackCount(u64),
    // Pull-model winnings
    Winner(u64, u32),              // (hackathon_id, rank) -> WinnerRecord
    WinnerByAddress(u64, Address), // (hackathon_id, address) -> u32 rank
    WinnerCount(u64),              // (hackathon_id) -> u32 winner count recorded at finalize
    ClaimWindow,                   // u64 seconds; admin-configurable, default = 90 days
}
