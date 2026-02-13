#![no_std]

pub mod contract;
pub mod error;
pub mod events;
pub mod storage;

#[cfg(test)]
mod tests;

pub use crate::contract::CrowdfundRegistry;
pub use crate::contract::CrowdfundRegistryClient;
