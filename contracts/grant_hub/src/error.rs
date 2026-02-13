use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotMilestoneGrant = 2,
    NotRecipient = 3,
    InvalidMilestoneStatus = 4,
    MilestoneNotFound = 5,
    MilestoneNotSubmitted = 6,
    NotRetrospectiveGrant = 7,
    NotQFGrant = 8,
    GrantNotFound = 9,
    NotInitialized = 14,
}
