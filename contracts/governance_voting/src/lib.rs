#![no_std]

pub mod contract;
pub mod error;
pub mod events;
pub mod storage;

#[cfg(test)]
mod tests;

pub use crate::contract::GovernanceVoting;
pub use crate::contract::GovernanceVotingClient;
