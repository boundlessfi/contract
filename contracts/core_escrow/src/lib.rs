#![no_std]
#![allow(clippy::needless_borrows_for_generic_args)]
#![allow(clippy::too_many_arguments)]

pub mod contract;
pub mod error;
pub mod events;
pub mod storage;

#[cfg(test)]
mod tests;

pub use crate::contract::CoreEscrow;
pub use crate::contract::CoreEscrowClient;
pub use crate::error::EscrowError as CoreEscrowError;
