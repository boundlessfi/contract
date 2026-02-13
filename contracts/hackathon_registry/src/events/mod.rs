use soroban_sdk::{contractevent, Address};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HackathonCreated {
    #[topic]
    pub id: u64,
    pub organizer: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackAdded {
    #[topic]
    pub hackathon_id: u64,
    pub track_id: u32,
    pub sponsor: Address,
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
pub struct JudgingFinalized {
    #[topic]
    pub hackathon_id: u64,
}
