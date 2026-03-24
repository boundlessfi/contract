use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

use boundless_types::ttl::{
    INSTANCE_TTL_EXTEND, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD,
};
use boundless_types::ModuleType;
use core_escrow::CoreEscrowClient;
use reputation_registry::ReputationRegistryClient;

use crate::error::Error;
use crate::events::{
    HackathonCancelled, HackathonCreated, PrizesDistributed, ProjectSubmitted, ScoreRecorded,
    SponsoredTrackAdded, TeamRegistered, TrackPrizesDistributed,
};
use crate::storage::{DataKey, Hackathon, HackathonStatus, SponsoredTrack, Submission};

#[contract]
pub struct HackathonRegistry;

#[contractimpl]
impl HackathonRegistry {
    // ========================================================================
    // INITIALIZATION
    // ========================================================================

    pub fn init(
        env: Env,
        admin: Address,
        core_escrow: Address,
        reputation_registry: Address,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::CoreEscrow, &core_escrow);
        env.storage()
            .instance()
            .set(&DataKey::ReputationRegistry, &reputation_registry);
        env.storage()
            .instance()
            .set(&DataKey::HackathonCount, &0u64);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    // ========================================================================
    // CREATION
    // ========================================================================

    pub fn create_hackathon(
        env: Env,
        creator: Address,
        title: String,
        metadata_cid: String,
        prize_pool: i128,
        asset: Address,
        registration_deadline: u64,
        submission_deadline: u64,
        judging_deadline: u64,
        max_participants: u32,
        prize_tiers: Vec<u32>,
    ) -> Result<u64, Error> {
        creator.require_auth();

        // Validate deadlines
        if registration_deadline >= submission_deadline
            || submission_deadline >= judging_deadline
        {
            return Err(Error::InvalidDeadlines);
        }

        // Validate prize_tiers sum to 10000 basis points
        let mut total_bps: u32 = 0;
        for i in 0..prize_tiers.len() {
            total_bps += prize_tiers.get(i).unwrap();
        }
        if total_bps != 10000 {
            return Err(Error::InvalidPrizeTiers);
        }

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::HackathonCount)
            .unwrap_or(0);
        count += 1;
        env.storage()
            .instance()
            .set(&DataKey::HackathonCount, &count);

        // Create escrow pool
        let esc_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;
        let esc_client = CoreEscrowClient::new(&env, &esc_addr);

        let pool_id = esc_client.create_pool(
            &creator,
            &ModuleType::Hackathon,
            &count,
            &prize_pool,
            &asset,
            &judging_deadline,
            &env.current_contract_address(),
        );

        // Lock the pool immediately
        esc_client.lock_pool(&pool_id);

        // Store prize tiers decomposed
        for i in 0..prize_tiers.len() {
            let pct = prize_tiers.get(i).unwrap();
            env.storage()
                .persistent()
                .set(&DataKey::PrizeTier(count, i), &pct);
        }

        let hackathon = Hackathon {
            id: count,
            creator: creator.clone(),
            title,
            metadata_cid,
            status: HackathonStatus::Registration,
            prize_pool,
            asset,
            pool_id,
            registration_deadline,
            submission_deadline,
            judging_deadline,
            judge_count: 0,
            submission_count: 0,
            max_participants,
        };

        let hack_key = DataKey::Hackathon(count);
        env.storage()
            .persistent()
            .set(&hack_key, &hackathon);
        Self::extend_persistent_ttl(&env, &hack_key);
        Self::extend_instance_ttl(&env);

        HackathonCreated {
            id: count,
            creator,
        }
        .publish(&env);

        Ok(count)
    }

    // ========================================================================
    // JUDGES
    // ========================================================================

    pub fn add_judge(env: Env, hackathon_id: u64, judge: Address) -> Result<(), Error> {
        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;
        hackathon.creator.require_auth();

        // Check not already a judge
        if env
            .storage()
            .persistent()
            .has(&DataKey::Judge(hackathon_id, judge.clone()))
        {
            return Err(Error::AlreadyJudge);
        }

        let idx = hackathon.judge_count;
        env.storage()
            .persistent()
            .set(&DataKey::Judge(hackathon_id, judge.clone()), &true);
        env.storage()
            .persistent()
            .set(&DataKey::JudgeIndex(hackathon_id, idx), &judge);

        hackathon.judge_count += 1;
        env.storage()
            .persistent()
            .set(&DataKey::Hackathon(hackathon_id), &hackathon);

        Ok(())
    }

    pub fn remove_judge(env: Env, hackathon_id: u64, judge: Address) -> Result<(), Error> {
        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;
        hackathon.creator.require_auth();

        if !env
            .storage()
            .persistent()
            .has(&DataKey::Judge(hackathon_id, judge.clone()))
        {
            return Err(Error::JudgeNotFound);
        }

        env.storage()
            .persistent()
            .remove(&DataKey::Judge(hackathon_id, judge.clone()));

        // Find and remove from index by swapping with last
        let last_idx = hackathon.judge_count - 1;
        for i in 0..hackathon.judge_count {
            let indexed: Address = env
                .storage()
                .persistent()
                .get(&DataKey::JudgeIndex(hackathon_id, i))
                .unwrap();
            if indexed == judge {
                if i != last_idx {
                    let last: Address = env
                        .storage()
                        .persistent()
                        .get(&DataKey::JudgeIndex(hackathon_id, last_idx))
                        .unwrap();
                    env.storage()
                        .persistent()
                        .set(&DataKey::JudgeIndex(hackathon_id, i), &last);
                }
                env.storage()
                    .persistent()
                    .remove(&DataKey::JudgeIndex(hackathon_id, last_idx));
                break;
            }
        }

        hackathon.judge_count -= 1;
        env.storage()
            .persistent()
            .set(&DataKey::Hackathon(hackathon_id), &hackathon);

        Ok(())
    }

    // ========================================================================
    // REGISTRATION & SUBMISSION
    // ========================================================================

    pub fn register_team(env: Env, hackathon_id: u64, team_lead: Address) -> Result<(), Error> {
        team_lead.require_auth();

        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;

        // Must be before registration deadline
        if env.ledger().timestamp() > hackathon.registration_deadline {
            return Err(Error::RegistrationClosed);
        }

        // Check not already registered
        if env
            .storage()
            .persistent()
            .has(&DataKey::Submission(hackathon_id, team_lead.clone()))
        {
            return Err(Error::AlreadyRegistered);
        }

        // Check max participants
        if hackathon.submission_count >= hackathon.max_participants {
            return Err(Error::MaxParticipantsReached);
        }

        // Create a placeholder submission (not yet submitted)
        let submission = Submission {
            team_lead: team_lead.clone(),
            metadata_cid: String::from_str(&env, ""),
            submitted_at: 0,
            total_score: 0,
            score_count: 0,
            disqualified: false,
        };

        let idx = hackathon.submission_count;
        env.storage()
            .persistent()
            .set(&DataKey::Submission(hackathon_id, team_lead.clone()), &submission);
        env.storage()
            .persistent()
            .set(&DataKey::SubmissionIndex(hackathon_id, idx), &team_lead.clone());

        hackathon.submission_count += 1;
        let hack_key = DataKey::Hackathon(hackathon_id);
        env.storage()
            .persistent()
            .set(&hack_key, &hackathon);
        Self::extend_persistent_ttl(&env, &hack_key);
        Self::extend_instance_ttl(&env);

        TeamRegistered {
            hackathon_id,
            team_lead,
        }
        .publish(&env);

        Ok(())
    }

    pub fn submit_project(
        env: Env,
        hackathon_id: u64,
        team_lead: Address,
        metadata_cid: String,
    ) -> Result<(), Error> {
        team_lead.require_auth();

        let hackathon = Self::load_hackathon(&env, hackathon_id)?;

        // Must be before submission deadline
        if env.ledger().timestamp() > hackathon.submission_deadline {
            return Err(Error::SubmissionClosed);
        }

        let sub_key = DataKey::Submission(hackathon_id, team_lead.clone());
        let mut submission: Submission = env
            .storage()
            .persistent()
            .get(&sub_key)
            .ok_or(Error::NotRegistered)?;

        // Check not already submitted
        if submission.submitted_at > 0 {
            return Err(Error::AlreadySubmitted);
        }

        submission.metadata_cid = metadata_cid;
        submission.submitted_at = env.ledger().timestamp();

        env.storage().persistent().set(&sub_key, &submission);

        ProjectSubmitted {
            hackathon_id,
            team_lead,
        }
        .publish(&env);

        Ok(())
    }

    // ========================================================================
    // JUDGING
    // ========================================================================

    /// Permissionless: transitions hackathon to Judging status after submission deadline.
    pub fn open_judging(env: Env, hackathon_id: u64) -> Result<(), Error> {
        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;

        if hackathon.status != HackathonStatus::Registration
            && hackathon.status != HackathonStatus::Submission
        {
            return Err(Error::InvalidStatus);
        }

        if env.ledger().timestamp() <= hackathon.submission_deadline {
            return Err(Error::SubmissionClosed);
        }

        hackathon.status = HackathonStatus::Judging;
        env.storage()
            .persistent()
            .set(&DataKey::Hackathon(hackathon_id), &hackathon);

        Ok(())
    }

    pub fn score_submission(
        env: Env,
        hackathon_id: u64,
        judge: Address,
        team_lead: Address,
        score: u32,
    ) -> Result<(), Error> {
        judge.require_auth();

        let hackathon = Self::load_hackathon(&env, hackathon_id)?;

        // Must be after submission deadline
        if env.ledger().timestamp() <= hackathon.submission_deadline {
            return Err(Error::JudgingNotActive);
        }

        // Must be before judging deadline
        if env.ledger().timestamp() > hackathon.judging_deadline {
            return Err(Error::JudgingNotActive);
        }

        // Score must be 0-100
        if score > 100 {
            return Err(Error::InvalidScore);
        }

        // Judge must be registered
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Judge(hackathon_id, judge.clone()))
        {
            return Err(Error::NotAJudge);
        }

        // Check submission exists
        let sub_key = DataKey::Submission(hackathon_id, team_lead.clone());
        let mut submission: Submission = env
            .storage()
            .persistent()
            .get(&sub_key)
            .ok_or(Error::SubmissionNotFound)?;

        // Check not already scored by this judge
        let score_key = DataKey::JudgeScore(hackathon_id, team_lead.clone(), judge.clone());
        if env.storage().persistent().has(&score_key) {
            return Err(Error::AlreadyScored);
        }

        // Record the score
        env.storage().persistent().set(&score_key, &score);

        // Update submission totals
        submission.total_score += score;
        submission.score_count += 1;
        env.storage().persistent().set(&sub_key, &submission);

        ScoreRecorded {
            hackathon_id,
            judge,
            team_lead,
            score,
        }
        .publish(&env);

        Ok(())
    }

    // ========================================================================
    // FINALIZATION
    // ========================================================================

    /// Permissionless: anyone can call after judging deadline to finalize and distribute prizes.
    pub fn finalize_hackathon(env: Env, hackathon_id: u64) -> Result<(), Error> {
        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;

        // Must be after judging deadline
        if env.ledger().timestamp() <= hackathon.judging_deadline {
            return Err(Error::JudgingNotOver);
        }

        if hackathon.status == HackathonStatus::Completed
            || hackathon.status == HackathonStatus::Cancelled
        {
            return Err(Error::InvalidStatus);
        }

        let esc_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;
        let esc_client = CoreEscrowClient::new(&env, &esc_addr);

        let rep_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::ReputationRegistry)
            .ok_or(Error::NotInitialized)?;
        let rep_client = ReputationRegistryClient::new(&env, &rep_addr);

        // Collect eligible submissions with avg scores
        // We'll build a sorted list by collecting into arrays
        let sub_count = hackathon.submission_count;
        if sub_count == 0 {
            return Err(Error::NoSubmissions);
        }

        // Gather submissions: (index_in_storage, avg_score, team_lead)
        // We use a simple bubble sort since submission counts are small
        let mut leads: Vec<Address> = Vec::new(&env);
        let mut scores: Vec<u32> = Vec::new(&env);

        for i in 0..sub_count {
            let lead: Address = env
                .storage()
                .persistent()
                .get(&DataKey::SubmissionIndex(hackathon_id, i))
                .unwrap();
            let sub: Submission = env
                .storage()
                .persistent()
                .get(&DataKey::Submission(hackathon_id, lead.clone()))
                .unwrap();

            // Skip disqualified
            if sub.disqualified {
                continue;
            }
            // Skip if no submission was made
            if sub.submitted_at == 0 {
                continue;
            }

            let avg = if sub.score_count > 0 {
                sub.total_score / sub.score_count
            } else {
                0
            };

            // Insert in sorted order (descending by score)
            let mut inserted = false;
            for j in 0..leads.len() {
                if avg > scores.get(j).unwrap() {
                    leads.insert(j, lead.clone());
                    scores.insert(j, avg);
                    inserted = true;
                    break;
                }
            }
            if !inserted {
                leads.push_back(lead.clone());
                scores.push_back(avg);
            }
        }

        // Distribute prizes based on prize_tiers
        // Count how many prize tiers exist
        let mut tier_count: u32 = 0;
        loop {
            if !env
                .storage()
                .persistent()
                .has(&DataKey::PrizeTier(hackathon_id, tier_count))
            {
                break;
            }
            tier_count += 1;
        }

        let num_winners = if leads.len() < tier_count {
            leads.len()
        } else {
            tier_count
        };

        for rank in 0..num_winners {
            let lead = leads.get(rank).unwrap();
            let pct: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::PrizeTier(hackathon_id, rank))
                .unwrap();

            let amount = hackathon.prize_pool
                .checked_mul(pct as i128)
                .ok_or(Error::Overflow)?
                / 10000;
            if amount > 0 {
                esc_client.release_partial(&hackathon.pool_id, &lead, &amount);
            }

            // Record hackathon result in reputation
            let is_win = rank == 0;
            let points = if rank == 0 {
                100u32
            } else if rank == 1 {
                50u32
            } else {
                25u32
            };
            rep_client.record_hackathon_result(
                &env.current_contract_address(),
                &lead,
                &points,
                &is_win,
            );
        }

        hackathon.status = HackathonStatus::Completed;
        env.storage()
            .persistent()
            .set(&DataKey::Hackathon(hackathon_id), &hackathon);

        PrizesDistributed { hackathon_id }.publish(&env);

        Ok(())
    }

    // ========================================================================
    // ADMIN
    // ========================================================================

    pub fn disqualify_submission(
        env: Env,
        hackathon_id: u64,
        team_lead: Address,
    ) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let sub_key = DataKey::Submission(hackathon_id, team_lead);
        let mut submission: Submission = env
            .storage()
            .persistent()
            .get(&sub_key)
            .ok_or(Error::SubmissionNotFound)?;

        if submission.disqualified {
            return Err(Error::AlreadyDisqualified);
        }

        submission.disqualified = true;
        env.storage().persistent().set(&sub_key, &submission);

        Ok(())
    }

    pub fn cancel_hackathon(env: Env, hackathon_id: u64) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;

        if hackathon.status == HackathonStatus::Completed
            || hackathon.status == HackathonStatus::Cancelled
        {
            return Err(Error::HackathonNotCancellable);
        }

        let esc_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;
        let esc_client = CoreEscrowClient::new(&env, &esc_addr);

        // Refund the entire prize pool
        esc_client.refund_all(&hackathon.pool_id);

        hackathon.status = HackathonStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Hackathon(hackathon_id), &hackathon);

        HackathonCancelled { hackathon_id }.publish(&env);

        Ok(())
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    // ========================================================================
    // SPONSORED TRACKS
    // ========================================================================

    pub fn add_sponsored_track(
        env: Env,
        hackathon_id: u64,
        sponsor: Address,
        track_name: String,
        prize_amount: i128,
        asset: Address,
    ) -> Result<u32, Error> {
        sponsor.require_auth();

        let hackathon = Self::load_hackathon(&env, hackathon_id)?;

        // Must be in Registration or Submission (open) status
        if hackathon.status != HackathonStatus::Registration
            && hackathon.status != HackathonStatus::Submission
        {
            return Err(Error::InvalidTrackStatus);
        }

        // Get and increment track count
        let track_count_key = DataKey::HackathonTrackCount(hackathon_id);
        let track_id: u32 = env
            .storage()
            .persistent()
            .get(&track_count_key)
            .unwrap_or(0);

        // Create escrow pool for this track
        let esc_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;
        let esc_client = CoreEscrowClient::new(&env, &esc_addr);

        let derived_module_id = hackathon_id * 1000 + track_id as u64;
        let pool_id = esc_client.create_pool(
            &sponsor,
            &ModuleType::Hackathon,
            &derived_module_id,
            &prize_amount,
            &asset,
            &hackathon.judging_deadline,
            &env.current_contract_address(),
        );

        // Lock the pool immediately
        esc_client.lock_pool(&pool_id);

        // Store track info
        let track = SponsoredTrack {
            track_id,
            hackathon_id,
            sponsor: sponsor.clone(),
            track_name,
            prize_amount,
            asset,
            pool_id,
        };

        let track_key = DataKey::HackathonTrack(hackathon_id, track_id);
        env.storage().persistent().set(&track_key, &track);
        Self::extend_persistent_ttl(&env, &track_key);

        // Increment track count
        env.storage()
            .persistent()
            .set(&track_count_key, &(track_id + 1));

        Self::extend_instance_ttl(&env);

        SponsoredTrackAdded {
            hackathon_id,
            track_id,
            sponsor,
        }
        .publish(&env);

        Ok(track_id)
    }

    pub fn distribute_track_prizes(
        env: Env,
        hackathon_id: u64,
        track_id: u32,
        winners: Vec<(Address, i128)>,
    ) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let hackathon = Self::load_hackathon(&env, hackathon_id)?;

        // Must be in Judging or Completed status
        if hackathon.status != HackathonStatus::Judging
            && hackathon.status != HackathonStatus::Completed
        {
            return Err(Error::InvalidStatus);
        }

        // Load the track
        let track_key = DataKey::HackathonTrack(hackathon_id, track_id);
        let track: SponsoredTrack = env
            .storage()
            .persistent()
            .get(&track_key)
            .ok_or(Error::TrackNotFound)?;

        let esc_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;
        let esc_client = CoreEscrowClient::new(&env, &esc_addr);

        let rep_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::ReputationRegistry)
            .ok_or(Error::NotInitialized)?;
        let rep_client = ReputationRegistryClient::new(&env, &rep_addr);

        // Distribute prizes to each winner
        for i in 0..winners.len() {
            let (winner, amount) = winners.get(i).unwrap();

            if amount > 0 {
                esc_client.release_partial(&track.pool_id, &winner, &amount);
            }

            // Record reputation for track winner
            let is_win = i == 0;
            let points = if i == 0 {
                100u32
            } else if i == 1 {
                50u32
            } else {
                25u32
            };
            rep_client.record_hackathon_result(
                &env.current_contract_address(),
                &winner,
                &points,
                &is_win,
            );
        }

        TrackPrizesDistributed {
            hackathon_id,
            track_id,
        }
        .publish(&env);

        Ok(())
    }

    // ========================================================================
    // QUERIES
    // ========================================================================

    pub fn get_hackathon(env: Env, id: u64) -> Result<Hackathon, Error> {
        let key = DataKey::Hackathon(id);
        let hackathon = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::HackathonNotFound)?;
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);
        Ok(hackathon)
    }

    pub fn get_submission(env: Env, hackathon_id: u64, team_lead: Address) -> Result<Submission, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Submission(hackathon_id, team_lead))
            .ok_or(Error::SubmissionNotFound)
    }

    // ========================================================================
    // INTERNAL
    // ========================================================================

    fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND);
    }

    fn extend_persistent_ttl(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND);
    }

    fn load_hackathon(env: &Env, id: u64) -> Result<Hackathon, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Hackathon(id))
            .ok_or(Error::HackathonNotFound)
    }
}
