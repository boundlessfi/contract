#![no_std]

pub mod contract;
pub mod error;
pub mod storage;

#[cfg(test)]
mod tests;

pub use crate::contract::SmartWallet;
pub use crate::contract::SmartWalletClient;
