use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 800,
    NotInitialized = 801,
    NotAuthorized = 802,
    CampaignNotFound = 803,
    DeadlinePassed = 804,
    NotCampaigning = 805,
    BelowMinPledge = 806,
    InvalidState = 807,
    MilestoneNotPending = 808,
    MilestoneNotFound = 809,
    MilestoneNotSubmitted = 810,
    CampaignAlreadyFunded = 811,
    CampaignActive = 812,
    NoPledgeFound = 813,
    AlreadyRefunded = 814,
    NotOwner = 815,
    InvalidMilestones = 816,
    RefundBatchDone = 817,
    DeadlineNotPassed = 818,
    AmountNotPositive = 819,
    Overflow = 820,
}
