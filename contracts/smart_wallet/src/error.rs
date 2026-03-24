use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1100,
    NotInitialized = 1101,
    NotOwner = 1102,
    InvalidSignature = 1103,
    SignerNotFound = 1104,
    SignerAlreadyExists = 1105,
    TooManySigners = 1106,
    NoValidSignature = 1107,
    InvalidPublicKey = 1108,
}
