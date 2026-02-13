#![no_std]

pub mod contract;
pub mod error;
pub mod events;
pub mod storage;

#[cfg(test)]
mod tests;

pub use crate::contract::CoreEscrow;
pub use crate::contract::CoreEscrowClient;
pub use crate::error::Error as CoreEscrowError;
pub use crate::storage::ModuleType;
