#![no_std]

pub mod contract;
pub mod error;
pub mod events;
pub mod math;
pub mod storage; // Ensure math is still accessible

#[cfg(test)]
mod tests;

pub use crate::contract::PaymentRouter;
pub use crate::contract::PaymentRouterClient;
pub use crate::storage::ModuleType;
