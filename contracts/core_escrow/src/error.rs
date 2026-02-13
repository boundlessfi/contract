use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotAuthorized = 2,
    PoolAlreadyExists = 3,
    PoolNotFound = 4,
    PoolLocked = 5,
    InvalidAsset = 6,
    InsufficientFunds = 7,
    SlotAlreadyReleased = 8,
    SlotNotFound = 9,
    SlotsExceedDeposit = 10,
    InvalidAmount = 11,
    NotInitialized = 12,
}
