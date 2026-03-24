use boundless_types::ActivityCategory;
use soroban_sdk::{contracttype, Address, BytesN, String};

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BountyType {
    FCFS,        // First-come-first-served: first to claim gets it
    Application, // Creator reviews applications, selects one
    Contest,     // Multiple submissions, creator picks winner(s)
    Split,       // Multiple contributors with defined shares
}

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BountyStatus {
    Open,
    InProgress,
    InReview,
    Completed,
    Cancelled,
}

#[contracttype]
#[derive(Clone)]
pub struct Bounty {
    pub id: u64,
    pub creator: Address,
    pub title: String,
    pub metadata_cid: String,
    pub bounty_type: BountyType,
    pub status: BountyStatus,
    pub amount: i128,
    pub asset: Address,
    pub category: ActivityCategory,
    pub created_at: u64,
    pub deadline: u64,
    pub assignee: Option<Address>,
    pub escrow_pool_id: BytesN<32>,
    pub winner_count: u32,
}

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApplicationStatus {
    Pending,
    Accepted,
    Rejected,
}

#[contracttype]
#[derive(Clone)]
pub struct Application {
    pub bounty_id: u64,
    pub applicant: Address,
    pub proposal: String,
    pub submitted_at: u64,
    pub status: ApplicationStatus,
}

#[contracttype]
pub enum DataKey {
    Admin,
    CoreEscrow,
    ReputationRegistry,
    BountyCount,
    Bounty(u64),
    Application(u64, Address),
    // Track applicant list per bounty for credit restoration
    ApplicantCount(u64),
    Applicant(u64, u32), // bounty_id, index -> Address
}
