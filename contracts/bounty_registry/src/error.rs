use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 700,
    NotInitialized = 701,
    NotAuthorized = 702,
    BountyNotFound = 703,
    BountyNotOpen = 704,
    DeadlinePassed = 705,
    AlreadyApplied = 706,
    ApplicationNotFound = 707,
    ApplicationNotPending = 708,
    InvalidRating = 709,
    NoEscrowPool = 710,
    AmountNotPositive = 711,
    NotCreator = 712,
    NotAssignee = 713,
    NotInProgress = 714,
    NotReviewable = 715,
    InvalidSubType = 716,
    AlreadyClaimed = 717,
    InsufficientCredits = 718,
    BountyNotCompleted = 719,
    InvalidSplitShares = 720,
    NotContestType = 721,
    NotSplitType = 722,
    SlotNotFound = 723,
    CannotCancel = 724,
    NotFCFSType = 725,
    AutoReleaseNotReady = 726,
}
