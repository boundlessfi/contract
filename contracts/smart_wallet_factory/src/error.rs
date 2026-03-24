use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1200,
    NotInitialized = 1201,
    NotAdmin = 1202,
    DeployFailed = 1203,
}
