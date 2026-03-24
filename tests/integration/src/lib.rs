// Integration test crate — no production code, only test modules.
#[cfg(test)]
mod setup;
#[cfg(test)]
mod test_bounty_e2e;
#[cfg(test)]
mod test_crowdfund_e2e;
#[cfg(test)]
mod test_grant_e2e;
#[cfg(test)]
mod test_hackathon_e2e;
#[cfg(test)]
mod test_cross_module;
