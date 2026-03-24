use soroban_sdk::{contractevent, Address};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectRegistered {
    #[topic]
    pub id: u64,
    pub owner: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationUpgraded {
    #[topic]
    pub project_id: u64,
    pub new_level: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectSuspended {
    #[topic]
    pub project_id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WarningIssued {
    #[topic]
    pub project_id: u64,
    pub warning_level: u32,
}
