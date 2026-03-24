#![no_std]
#![allow(clippy::needless_borrows_for_generic_args)]
pub mod contract;
pub mod error;
pub mod events;
pub mod storage;

#[cfg(test)]
mod tests;

pub use crate::contract::ProjectRegistry;
pub use crate::contract::ProjectRegistryClient;
