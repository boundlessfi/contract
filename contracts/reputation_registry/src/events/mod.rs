use soroban_sdk::{contractevent, Address};

#[contractevent(topics = ["ScoreUpdated"], data_format = "vec")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreUpdated {
    #[topic]
    pub contributor: Address,
    pub overall_score: u32,
    pub level: u32,
}

#[contractevent(topics = ["ModuleAuthorized"], data_format = "single-value")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleAuthorized {
    #[topic]
    pub module: Address,
    pub authorized: bool,
}
