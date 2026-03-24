use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 600,
    NotInitialized = 601,
    NotAuthorized = 602,
    ProjectNotFound = 603,
    ProjectSuspended = 604,
    BudgetExceedsLimit = 605,
    ModuleNotAuthorized = 606,
    InsufficientDeposit = 607,
    NoDepositHeld = 608,
    InvalidAmount = 609,
    Overflow = 610,
}
