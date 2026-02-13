use soroban_sdk::{contractevent, Address};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BountyCreated {
    #[topic]
    pub bounty_id: u64,
    pub creator: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BountyApplied {
    #[topic]
    pub bounty_id: u64,
    pub applicant: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BountyAssigned {
    #[topic]
    pub bounty_id: u64,
    pub assignee: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkSubmitted {
    #[topic]
    pub bounty_id: u64,
    pub contributor: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmissionAccepted {
    #[topic]
    pub bounty_id: u64,
    pub assignee: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BountyCancelled {
    #[topic]
    pub bounty_id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationRejected {
    #[topic]
    pub bounty_id: u64,
    pub applicant: Address,
}
