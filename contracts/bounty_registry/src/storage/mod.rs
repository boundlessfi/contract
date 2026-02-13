use reputation_registry::ActivityCategory;
use soroban_sdk::{contracttype, Address, BytesN, String};

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BountyType {
    Permissioned, // Creator assigns a specific worker (formerly Application)
    Contest,      // Competitive submission; creator picks winner(s)
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
    pub model: BountyType,
    pub status: BountyStatus,
    pub amount: i128,
    pub asset: Address,
    pub category: ActivityCategory,
    pub created_at: u64,
    pub deadline: u64,
    pub assignee: Option<Address>,
    pub escrow_pool_id: Option<BytesN<32>>,
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
    Application(u64, Address), // bounty_id, applicant -> Application
}
