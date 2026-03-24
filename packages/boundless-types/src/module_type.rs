use soroban_sdk::contracttype;

/// Identifies which platform module owns a resource (escrow pool, fee record, etc.)
#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleType {
    Bounty,
    Crowdfund,
    Grant,
    Hackathon,
}

/// Granular sub-type for fee rate lookup and audit trail.
#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubType {
    BountyFCFS,
    BountyApplication,
    BountyContest,
    BountySplit,
    CrowdfundPledge,
    GrantMilestone,
    GrantRetrospective,
    GrantQFMatchingPool,
    HackathonMain,
    HackathonTrack,
}

impl SubType {
    /// Map a SubType to its parent ModuleType.
    pub fn module(&self) -> ModuleType {
        match self {
            SubType::BountyFCFS
            | SubType::BountyApplication
            | SubType::BountyContest
            | SubType::BountySplit => ModuleType::Bounty,
            SubType::CrowdfundPledge => ModuleType::Crowdfund,
            SubType::GrantMilestone
            | SubType::GrantRetrospective
            | SubType::GrantQFMatchingPool => ModuleType::Grant,
            SubType::HackathonMain | SubType::HackathonTrack => ModuleType::Hackathon,
        }
    }
}
