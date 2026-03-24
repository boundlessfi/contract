use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 900,
    NotInitialized = 901,
    GrantNotFound = 902,
    NotMilestoneGrant = 903,
    NotRetrospectiveGrant = 904,
    NotQFGrant = 905,
    NotRecipient = 906,
    MilestoneNotFound = 907,
    InvalidMilestoneStatus = 908,
    MilestoneNotSubmitted = 909,
    InvalidAmount = 910,
    InvalidMilestonePercents = 911,
    GrantNotActive = 912,
    VotingNotEnded = 913,
    NoVoteSession = 914,
    InvalidProjectIndex = 915,
    CannotCancel = 916,
    NotCreator = 917,
    Overflow = 918,
}
