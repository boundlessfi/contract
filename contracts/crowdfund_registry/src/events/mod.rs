use crate::storage::{DisputeResolution, VoteRejectionReason};
use soroban_sdk::{contractevent, Address, BytesN};

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
pub struct MilestoneRejected {
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
pub struct CampaignCancelledByOwner {
    #[topic]
    pub id: u64,
}

/// Emitted after each `process_refund_batch` call.
/// `count` is the number of backers refunded in this call.
/// The backend tracks batch progress; no batch_index is stored on-chain.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefundBatchProcessed {
    #[topic]
    pub campaign_id: u64,
    pub count: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneDisputed {
    #[topic]
    pub campaign_id: u64,
    pub milestone_id: u32,
    pub disputer: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignTerminated {
    #[topic]
    pub id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneOverdue {
    #[topic]
    pub campaign_id: u64,
    pub milestone_id: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneEscalated {
    #[topic]
    pub campaign_id: u64,
    pub milestone_id: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignSubmittedForReview {
    #[topic]
    pub id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignApproved {
    #[topic]
    pub id: u64,
    pub vote_session_id: BytesN<32>,
}

/// Emitted when an admin explicitly rejects a campaign before the vote stage.
/// The rejection reason is stored in the backend database, not on-chain.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignRejected {
    #[topic]
    pub id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignUpdated {
    #[topic]
    pub id: u64,
    pub funding_goal: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignValidated {
    #[topic]
    pub id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MilestoneRevisionRequested {
    #[topic]
    pub campaign_id: u64,
    pub milestone_id: u32,
}

/// Emitted when a community vote results in campaign rejection.
/// `reason` distinguishes the two rejection paths so the indexer
/// does not need to parse strings.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CampaignVoteRejected {
    #[topic]
    pub id: u64,
    pub reason: VoteRejectionReason,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisputeResolved {
    #[topic]
    pub campaign_id: u64,
    pub milestone_id: u32,
    pub resolution: DisputeResolution,
}
