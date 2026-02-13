use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotAuthorized = 2,
    SessionCollision = 3,
    SessionNotFound = 4,
    SessionNotActive = 5,
    VotingNotStarted = 6,
    VotingEnded = 7,
    AlreadyVoted = 8,
    OptionNotFound = 9,
    VotingInProgress = 10,
    NotQFRound = 11,
    RoundNotEnded = 12,
    NotInitialized = 13,
}
