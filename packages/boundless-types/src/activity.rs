use soroban_sdk::contracttype;

/// Skill/activity categories used across reputation scoring and bounty tagging.
#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActivityCategory {
    Development,
    Design,
    Marketing,
    Security,
    Community,
}
