use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotAuthorized = 2,
    BountyNotFound = 3,
    BountyNotOpen = 4,
    BountyDeadlinePassed = 5,
    AlreadyApplied = 6,
    ApplicationNotFound = 7,
    ApplicationNotPending = 8,
    InvalidRating = 9,
    NoEscrowPool = 10,
    AmountNotPositive = 11,
    NotCreator = 12,
    NotAssignee = 13,
    NotInProgress = 14,
    MustApplyBeforeSubmitting = 15,
    NotReviewable = 16,
    ActionOnlyForPermissioned = 17,
    NotInitialized = 18,
}
