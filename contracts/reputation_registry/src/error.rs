use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ReputationError {
    AlreadyInitialized = 300,
    NotInitialized = 301,
    ProfileNotFound = 302,
    ModuleNotAuthorized = 303,
    InsufficientCredits = 304,
    RechargeNotReady = 305,
}
