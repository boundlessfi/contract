use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    ProjectNotFound = 2,
    ProjectSuspended = 3,
    InsufficientDeposit = 4,
    UnauthorizedCaller = 5,
    NotInitialized = 6,
}
