use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    HackathonNotFound = 2,
    SubmissionClosed = 3,
    UnauthorizedJudge = 4,
    JudgingNotActive = 5,
    JudgingPeriodNotOver = 11,
    NoSubmissions = 12,
    NotInitialized = 13,
    NotFound = 8,
}
