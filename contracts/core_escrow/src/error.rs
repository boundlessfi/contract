use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum EscrowError {
    AlreadyInitialized = 100,
    NotInitialized = 101,
    NotAuthorized = 102,
    PoolNotFound = 103,
    PoolAlreadyExists = 104,
    PoolLocked = 105,
    PoolNotLocked = 106,
    InvalidAsset = 107,
    InsufficientFunds = 108,
    SlotNotFound = 109,
    SlotAlreadyReleased = 110,
    SlotsExceedDeposit = 111,
    InvalidAmount = 112,
    RateExceedsLimit = 113,
    RoutingPaused = 114,
    InsuranceCutOutOfRange = 115,
    Overflow = 116,
    ModuleNotAuthorized = 117,
    PoolExpired = 118,
    InsuranceInsufficient = 119,
}
