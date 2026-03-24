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
#[derive(Clone)]
pub enum DataKey {
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
}
