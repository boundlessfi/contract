use soroban_sdk::{contractevent, Address, String};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreUpdated {
    #[topic]
    pub contributor: Address,
    pub overall_score: u32,
    pub level: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleAuthorized {
    #[topic]
    pub module: Address,
    pub authorized: bool,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreditsSpent {
    #[topic]
    pub user: Address,
    pub remaining: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreditsAwarded {
    #[topic]
    pub user: Address,
    pub amount: u32,
    pub remaining: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreditsRecharged {
    #[topic]
    pub user: Address,
    pub remaining: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FraudRecorded {
    #[topic]
    pub contributor: Address,
    pub overall_score: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommunityBonusAdded {
    #[topic]
    pub contributor: Address,
    pub reason: String,
    pub points: u32,
}
