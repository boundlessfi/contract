use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1000,
    NotInitialized = 1001,
    HackathonNotFound = 1002,
    InvalidPrizeTiers = 1003,
    InvalidDeadlines = 1004,
    RegistrationClosed = 1005,
    MaxParticipantsReached = 1006,
    AlreadyRegistered = 1007,
    NotRegistered = 1008,
    SubmissionClosed = 1009,
    AlreadySubmitted = 1010,
    SubmissionNotFound = 1011,
    JudgingNotActive = 1012,
    NotAJudge = 1013,
    AlreadyScored = 1014,
    InvalidScore = 1015,
    JudgingNotOver = 1016,
    NoSubmissions = 1017,
    NotCreator = 1018,
    NotAdmin = 1019,
    InvalidStatus = 1020,
    AlreadyJudge = 1022,
    JudgeNotFound = 1023,
    AlreadyDisqualified = 1024,
    HackathonNotCancellable = 1025,
    TrackNotFound = 1026,
    InvalidTrackStatus = 1027,
}
