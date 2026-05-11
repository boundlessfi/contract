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

/// Legacy event. Still emitted by `distribute_track_prizes` (push-model for
/// sponsored tracks). Main-pool prizes now emit `HackathonFinalized` +
/// `PrizeClaimed` (pull model).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrizesDistributed {
    #[topic]
    pub hackathon_id: u64,
}

/// Emitted by `finalize_hackathon`. Winner records are written; no funds move.
/// Winners must call `claim_prize` to receive their tranche.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HackathonFinalized {
    #[topic]
    pub hackathon_id: u64,
    pub winner_count: u32,
}

/// Emitted by `claim_prize` each time a winner successfully pulls their prize.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrizeClaimed {
    #[topic]
    pub hackathon_id: u64,
    #[topic]
    pub winner: Address,
    pub rank: u32,
    pub amount: i128,
}

/// Emitted by `reclaim_unclaimed_prizes` when the creator sweeps prizes that
/// passed their claim_deadline without being claimed.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnclaimedPrizesReclaimed {
    #[topic]
    pub hackathon_id: u64,
    pub total_amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HackathonCancelled {
    #[topic]
    pub hackathon_id: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SponsoredTrackAdded {
    #[topic]
    pub hackathon_id: u64,
    pub track_id: u32,
    pub sponsor: Address,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackPrizesDistributed {
    #[topic]
    pub hackathon_id: u64,
    pub track_id: u32,
}
