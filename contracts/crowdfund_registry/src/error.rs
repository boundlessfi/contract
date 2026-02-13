use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    CampaignNotFound = 2,
    DeadlinePassed = 3,
    NotCampaigning = 4,
    BelowMinPledge = 5,
    InvalidState = 6,
    MilestoneNotPending = 7,
    MilestoneNotFound = 8,
    MilestoneNotSubmitted = 9,
    MilestoneAmountZero = 10,
    CampaignAlreadyFunded = 11,
    CampaignActive = 12,
    NoPledgeFound = 13,
    NotInitialized = 14,
    AlreadyRefunded = 15,
    NotAuthorized = 16,
}
