use soroban_sdk::contracttype;

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleType {
    Bounty,
    Crowdfund,
    Grant,
    Hackathon,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Treasury,
    CoreEscrow,
    FeeAccount,
    FeeRate(ModuleType),
}
