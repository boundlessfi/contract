use soroban_sdk::{
    contract, contractimpl, Address, BytesN, Env, IntoVal, String, Symbol, Val, Vec,
};

use boundless_types::ttl::{
    INSTANCE_TTL_EXTEND, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD,
};
use boundless_types::ModuleType;

use crate::error::Error;
use crate::events::{
    HackathonCancelled, HackathonCreated, PrizesDistributed, ProjectSubmitted, ScoreRecorded,
    SponsoredTrackAdded, TeamRegistered, TrackPrizesDistributed,
};
use crate::storage::{DataKey, Hackathon, HackathonStatus, SponsoredTrack, Submission};

fn sym(env: &Env, name: &str) -> Symbol {
    Symbol::new(env, name)
}

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

        if registration_deadline >= submission_deadline || submission_deadline >= judging_deadline {
            return Err(Error::InvalidDeadlines);
        }

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
        let escrow_addr = Self::get_escrow_addr(&env)?;
        let pool_args: Vec<Val> = Vec::from_array(
            &env,
            [
                creator.clone().into_val(&env),
                ModuleType::Hackathon.into_val(&env),
                count.into_val(&env),
                prize_pool.into_val(&env),
                asset.clone().into_val(&env),
                judging_deadline.into_val(&env),
                env.current_contract_address().into_val(&env),
            ],
        );
        let pool_id: BytesN<32> =
            env.invoke_contract(&escrow_addr, &sym(&env, "create_pool"), pool_args);

        // Lock the pool immediately
        let lock_args: Vec<Val> = Vec::from_array(&env, [pool_id.clone().into_val(&env)]);
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "lock_pool"), lock_args);

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
        env.storage().persistent().set(&hack_key, &hackathon);
        Self::extend_persistent_ttl(&env, &hack_key);
        Self::extend_instance_ttl(&env);

        HackathonCreated { id: count, creator }.publish(&env);

        Ok(count)
    }

    // ========================================================================
    // JUDGES
    // ========================================================================

    pub fn add_judge(env: Env, hackathon_id: u64, judge: Address) -> Result<(), Error> {
        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;
        hackathon.creator.require_auth();

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

        if env.ledger().timestamp() > hackathon.registration_deadline {
            return Err(Error::RegistrationClosed);
        }

        if env
            .storage()
            .persistent()
            .has(&DataKey::Submission(hackathon_id, team_lead.clone()))
        {
            return Err(Error::AlreadyRegistered);
        }

        if hackathon.submission_count >= hackathon.max_participants {
            return Err(Error::MaxParticipantsReached);
        }

        let submission = Submission {
            team_lead: team_lead.clone(),
            metadata_cid: String::from_str(&env, ""),
            submitted_at: 0,
            total_score: 0,
            score_count: 0,
            disqualified: false,
        };

        let idx = hackathon.submission_count;
        env.storage().persistent().set(
            &DataKey::Submission(hackathon_id, team_lead.clone()),
            &submission,
        );
        env.storage().persistent().set(
            &DataKey::SubmissionIndex(hackathon_id, idx),
            &team_lead.clone(),
        );

        hackathon.submission_count += 1;
        let hack_key = DataKey::Hackathon(hackathon_id);
        env.storage().persistent().set(&hack_key, &hackathon);
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

        if env.ledger().timestamp() > hackathon.submission_deadline {
            return Err(Error::SubmissionClosed);
        }

        let sub_key = DataKey::Submission(hackathon_id, team_lead.clone());
        let mut submission: Submission = env
            .storage()
            .persistent()
            .get(&sub_key)
            .ok_or(Error::NotRegistered)?;

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

    pub fn open_judging(env: Env, hackathon_id: u64) -> Result<(), Error> {
        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;

        if hackathon.status != HackathonStatus::Registration
            && hackathon.status != HackathonStatus::Submission
        {
            return Err(Error::InvalidStatus);
        }

        if env.ledger().timestamp() <= hackathon.submission_deadline {
            return Err(Error::SubmissionPeriodNotEnded);
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

        if hackathon.status != HackathonStatus::Judging {
            return Err(Error::JudgingNotActive);
        }
        if env.ledger().timestamp() > hackathon.judging_deadline {
            return Err(Error::JudgingNotActive);
        }
        if score > 100 {
            return Err(Error::InvalidScore);
        }

        if !env
            .storage()
            .persistent()
            .has(&DataKey::Judge(hackathon_id, judge.clone()))
        {
            return Err(Error::NotAJudge);
        }

        let sub_key = DataKey::Submission(hackathon_id, team_lead.clone());
        let mut submission: Submission = env
            .storage()
            .persistent()
            .get(&sub_key)
            .ok_or(Error::SubmissionNotFound)?;

        let score_key = DataKey::JudgeScore(hackathon_id, team_lead.clone(), judge.clone());
        if env.storage().persistent().has(&score_key) {
            return Err(Error::AlreadyScored);
        }

        env.storage().persistent().set(&score_key, &score);

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

    pub fn finalize_hackathon(env: Env, hackathon_id: u64) -> Result<(), Error> {
        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;

        if env.ledger().timestamp() <= hackathon.judging_deadline {
            return Err(Error::JudgingNotOver);
        }

        if hackathon.status == HackathonStatus::Completed
            || hackathon.status == HackathonStatus::Cancelled
        {
            return Err(Error::InvalidStatus);
        }

        let escrow_addr = Self::get_escrow_addr(&env)?;
        let rep_addr = Self::get_rep_addr(&env)?;

        let sub_count = hackathon.submission_count;
        if sub_count == 0 {
            return Err(Error::NoSubmissions);
        }

        // Gather submissions sorted by avg score (descending)
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

            if sub.disqualified || sub.submitted_at == 0 {
                continue;
            }

            let avg = if sub.score_count > 0 {
                sub.total_score / sub.score_count
            } else {
                0
            };

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

        // Count prize tiers
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

        let contract_addr = env.current_contract_address();

        for rank in 0..num_winners {
            let lead = leads.get(rank).unwrap();
            let pct: u32 = env
                .storage()
                .persistent()
                .get(&DataKey::PrizeTier(hackathon_id, rank))
                .unwrap();

            let amount = hackathon
                .prize_pool
                .checked_mul(pct as i128)
                .ok_or(Error::Overflow)?
                / 10000;
            if amount > 0 {
                let release_args: Vec<Val> = Vec::from_array(
                    &env,
                    [
                        hackathon.pool_id.clone().into_val(&env),
                        lead.clone().into_val(&env),
                        amount.into_val(&env),
                    ],
                );
                env.invoke_contract::<()>(
                    &escrow_addr,
                    &sym(&env, "release_partial"),
                    release_args,
                );
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
            let rep_args: Vec<Val> = Vec::from_array(
                &env,
                [
                    contract_addr.clone().into_val(&env),
                    lead.into_val(&env),
                    points.into_val(&env),
                    is_win.into_val(&env),
                ],
            );
            env.invoke_contract::<()>(&rep_addr, &sym(&env, "record_hackathon_result"), rep_args);
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

        let escrow_addr = Self::get_escrow_addr(&env)?;
        let refund_args: Vec<Val> =
            Vec::from_array(&env, [hackathon.pool_id.clone().into_val(&env)]);
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "refund_all"), refund_args);

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

        if hackathon.status != HackathonStatus::Registration
            && hackathon.status != HackathonStatus::Submission
        {
            return Err(Error::InvalidTrackStatus);
        }

        let track_count_key = DataKey::HackathonTrackCount(hackathon_id);
        let track_id: u32 = env
            .storage()
            .persistent()
            .get(&track_count_key)
            .unwrap_or(0);

        let escrow_addr = Self::get_escrow_addr(&env)?;
        let derived_module_id = hackathon_id * 1000 + track_id as u64;
        let pool_args: Vec<Val> = Vec::from_array(
            &env,
            [
                sponsor.clone().into_val(&env),
                ModuleType::Hackathon.into_val(&env),
                derived_module_id.into_val(&env),
                prize_amount.into_val(&env),
                asset.clone().into_val(&env),
                hackathon.judging_deadline.into_val(&env),
                env.current_contract_address().into_val(&env),
            ],
        );
        let pool_id: BytesN<32> =
            env.invoke_contract(&escrow_addr, &sym(&env, "create_pool"), pool_args);

        let lock_args: Vec<Val> = Vec::from_array(&env, [pool_id.clone().into_val(&env)]);
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "lock_pool"), lock_args);

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

        if hackathon.status != HackathonStatus::Judging
            && hackathon.status != HackathonStatus::Completed
        {
            return Err(Error::InvalidStatus);
        }

        let track_key = DataKey::HackathonTrack(hackathon_id, track_id);
        let track: SponsoredTrack = env
            .storage()
            .persistent()
            .get(&track_key)
            .ok_or(Error::TrackNotFound)?;

        let escrow_addr = Self::get_escrow_addr(&env)?;
        let rep_addr = Self::get_rep_addr(&env)?;
        let contract_addr = env.current_contract_address();

        for i in 0..winners.len() {
            let (winner, amount) = winners.get(i).unwrap();

            if amount > 0 {
                let release_args: Vec<Val> = Vec::from_array(
                    &env,
                    [
                        track.pool_id.clone().into_val(&env),
                        winner.clone().into_val(&env),
                        amount.into_val(&env),
                    ],
                );
                env.invoke_contract::<()>(
                    &escrow_addr,
                    &sym(&env, "release_partial"),
                    release_args,
                );
            }

            let is_win = i == 0;
            let points = if i == 0 {
                100u32
            } else if i == 1 {
                50u32
            } else {
                25u32
            };
            let rep_args: Vec<Val> = Vec::from_array(
                &env,
                [
                    contract_addr.clone().into_val(&env),
                    winner.into_val(&env),
                    points.into_val(&env),
                    is_win.into_val(&env),
                ],
            );
            env.invoke_contract::<()>(&rep_addr, &sym(&env, "record_hackathon_result"), rep_args);
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

    pub fn get_submission(
        env: Env,
        hackathon_id: u64,
        team_lead: Address,
    ) -> Result<Submission, Error> {
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

    fn get_escrow_addr(env: &Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)
    }

    fn get_rep_addr(env: &Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::ReputationRegistry)
            .ok_or(Error::NotInitialized)
    }
}
