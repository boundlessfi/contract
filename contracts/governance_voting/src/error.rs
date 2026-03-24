use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 500,
    NotInitialized = 501,
    NotAuthorized = 502,
    ModuleNotAuthorized = 503,
    SessionNotFound = 504,
    SessionNotActive = 505,
    AlreadyVoted = 506,
    InvalidOption = 507,
    SessionNotEnded = 508,
    VotingNotStarted = 509,
    InvalidTimeRange = 510,
}
