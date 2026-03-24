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
pub enum HackathonType {
    Traditional,
    SponsoredTracks,
}

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HackathonStatus {
    Draft,
    Published,
    Active,
    Judging,
    Distributing,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrizeTier {
    pub rank: u32,
    pub pct: u32, // bases points (e.g. 6000 = 60%)
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hackathon {
    pub id: u64,
    pub organizer: Address,
    pub project_id: u64,
    pub metadata_cid: String,
    pub status: HackathonStatus,
    pub main_pool_id: BytesN<32>,
    pub asset: Address,
    pub judges: Vec<Address>,
    pub submission_deadline: u64,
    pub judging_deadline: u64,
    pub prize_distribution: Vec<PrizeTier>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HackathonTrack {
    pub id: u32,
    pub name: String,
    pub sponsor: Address,
    pub prize_pool: i128,
    pub pool_id: BytesN<32>,
    pub prize_distribution: Vec<PrizeTier>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HackathonSubmission {
    pub team_lead: Address,
    pub members: Vec<Address>,
    pub project_name: String,
    pub submission_cid: String,
    pub track_ids: Vec<u32>,
    pub final_score: u32, // Weighted average * 100
    pub rank: u32,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    ProjectRegistry,
    CoreEscrow,
    VotingContract,
    ReputationRegistry,
    FeeAccount,
    Treasury,
    HackathonCount,
    Hackathon(u64),
    Track(u64, u32), // hackathon_id, track_id
    TrackCount(u64),
    Submission(u64, Address),          // hackathon_id, team_lead
    SubmissionList(u64),               // hackathon_id -> Vec<Address> (leads)
    JudgeScore(u64, Address, Address), // hackathon_id, judge, lead -> score
}
