use soroban_sdk::{contractevent, Address};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HackathonCreated {
    #[topic]
    pub id: u64,
    pub creator: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TeamRegistered {
    #[topic]
    pub hackathon_id: u64,
    #[topic]
    pub team_lead: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectSubmitted {
    #[topic]
    pub hackathon_id: u64,
    #[topic]
    pub team_lead: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoreRecorded {
    #[topic]
    pub hackathon_id: u64,
    pub judge: Address,
    pub team_lead: Address,
    pub score: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrizesDistributed {
    #[topic]
    pub hackathon_id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HackathonCancelled {
    #[topic]
    pub hackathon_id: u64,
}
