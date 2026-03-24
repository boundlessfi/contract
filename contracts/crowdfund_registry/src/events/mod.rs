use soroban_sdk::{contractevent, Address};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignCreated {
    #[topic]
    pub id: u64,
    pub owner: Address,
    pub funding_goal: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PledgeRecorded {
    #[topic]
    pub campaign_id: u64,
    #[topic]
    pub donor: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignFunded {
    #[topic]
    pub id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneSubmitted {
    #[topic]
    pub campaign_id: u64,
    pub milestone_id: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneApproved {
    #[topic]
    pub campaign_id: u64,
    pub milestone_id: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignFailed {
    #[topic]
    pub id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignCancelled {
    #[topic]
    pub id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefundBatchProcessed {
    #[topic]
    pub campaign_id: u64,
    pub batch_index: u32,
    pub count: u32,
}
