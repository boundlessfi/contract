use soroban_sdk::{
    contract, contractimpl, Address, BytesN, Env, IntoVal, String, Symbol, Val, Vec,
};

use boundless_types::ttl::{
    INSTANCE_TTL_EXTEND, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD,
};
use boundless_types::ModuleType;

use crate::error::HackathonError;
use crate::events::{
    HackathonCancelled, HackathonCreated, HackathonFinalized, PrizeClaimed, ProjectSubmitted,
    ScoreRecorded, SponsoredTrackAdded, TeamRegistered, TrackPrizesDistributed,
    UnclaimedPrizesReclaimed,
};
use crate::storage::{
    Hackathon, HackathonDataKey, HackathonStatus, SponsoredTrack, Submission, WinnerRecord,
};

/// Default claim window for unclaimed prizes if admin has not configured one:
/// 90 days = 90 * 24 * 60 * 60 seconds.
const DEFAULT_CLAIM_WINDOW_SECONDS: u64 = 90 * 86_400;

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
    ) -> Result<(), HackathonError> {
        if env.storage().instance().has(&HackathonDataKey::Admin) {
            return Err(HackathonError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage()
            .instance()
            .set(&HackathonDataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&HackathonDataKey::CoreEscrow, &core_escrow);
        env.storage()
            .instance()
            .set(&HackathonDataKey::ReputationRegistry, &reputation_registry);
        env.storage()
            .instance()
            .set(&HackathonDataKey::HackathonCount, &0u64);
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
    ) -> Result<u64, HackathonError> {
        creator.require_auth();

        if registration_deadline >= submission_deadline || submission_deadline >= judging_deadline {
            return Err(HackathonError::InvalidDeadlines);
        }

        let mut total_bps: u32 = 0;
        for i in 0..prize_tiers.len() {
            total_bps += prize_tiers.get(i).unwrap();
        }
        if total_bps != 10000 {
            return Err(HackathonError::InvalidPrizeTiers);
        }

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&HackathonDataKey::HackathonCount)
            .unwrap_or(0);
        count += 1;
        env.storage()
            .instance()
            .set(&HackathonDataKey::HackathonCount, &count);

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
                .set(&HackathonDataKey::PrizeTier(count, i), &pct);
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

        let hack_key = HackathonDataKey::Hackathon(count);
        env.storage().persistent().set(&hack_key, &hackathon);
        Self::extend_persistent_ttl(&env, &hack_key);
        Self::extend_instance_ttl(&env);

        HackathonCreated { id: count, creator }.publish(&env);

        Ok(count)
    }

    /// Alias for `create_hackathon`. Semantically identical; the name makes the
    /// cost model explicit to integrators — calling this transfers `prize_pool`
    /// from `creator` into escrow atomically with the hackathon creation.
    ///
    /// Smart-account creators are supported automatically: `creator.require_auth()`
    /// routes to `__check_auth` on contract addresses.
    pub fn create_and_fund_hackathon(
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
    ) -> Result<u64, HackathonError> {
        Self::create_hackathon(
            env,
            creator,
            title,
            metadata_cid,
            prize_pool,
            asset,
            registration_deadline,
            submission_deadline,
            judging_deadline,
            max_participants,
            prize_tiers,
        )
    }

    // ========================================================================
    // JUDGES
    // ========================================================================

    pub fn add_judge(env: Env, hackathon_id: u64, judge: Address) -> Result<(), HackathonError> {
        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;
        hackathon.creator.require_auth();

        if env
            .storage()
            .persistent()
            .has(&HackathonDataKey::Judge(hackathon_id, judge.clone()))
        {
            return Err(HackathonError::AlreadyJudge);
        }

        let idx = hackathon.judge_count;
        env.storage()
            .persistent()
            .set(&HackathonDataKey::Judge(hackathon_id, judge.clone()), &true);
        env.storage()
            .persistent()
            .set(&HackathonDataKey::JudgeIndex(hackathon_id, idx), &judge);

        hackathon.judge_count += 1;
        env.storage()
            .persistent()
            .set(&HackathonDataKey::Hackathon(hackathon_id), &hackathon);

        Ok(())
    }

    pub fn remove_judge(env: Env, hackathon_id: u64, judge: Address) -> Result<(), HackathonError> {
        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;
        hackathon.creator.require_auth();

        if !env
            .storage()
            .persistent()
            .has(&HackathonDataKey::Judge(hackathon_id, judge.clone()))
        {
            return Err(HackathonError::JudgeNotFound);
        }

        env.storage()
            .persistent()
            .remove(&HackathonDataKey::Judge(hackathon_id, judge.clone()));

        let last_idx = hackathon.judge_count - 1;
        for i in 0..hackathon.judge_count {
            let indexed: Address = env
                .storage()
                .persistent()
                .get(&HackathonDataKey::JudgeIndex(hackathon_id, i))
                .unwrap();
            if indexed == judge {
                if i != last_idx {
                    let last: Address = env
                        .storage()
                        .persistent()
                        .get(&HackathonDataKey::JudgeIndex(hackathon_id, last_idx))
                        .unwrap();
                    env.storage()
                        .persistent()
                        .set(&HackathonDataKey::JudgeIndex(hackathon_id, i), &last);
                }
                env.storage()
                    .persistent()
                    .remove(&HackathonDataKey::JudgeIndex(hackathon_id, last_idx));
                break;
            }
        }

        hackathon.judge_count -= 1;
        env.storage()
            .persistent()
            .set(&HackathonDataKey::Hackathon(hackathon_id), &hackathon);

        Ok(())
    }

    // ========================================================================
    // REGISTRATION & SUBMISSION
    // ========================================================================

    pub fn register_team(
        env: Env,
        hackathon_id: u64,
        team_lead: Address,
    ) -> Result<(), HackathonError> {
        team_lead.require_auth();

        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;

        if env.ledger().timestamp() > hackathon.registration_deadline {
            return Err(HackathonError::RegistrationClosed);
        }

        if env
            .storage()
            .persistent()
            .has(&HackathonDataKey::Submission(
                hackathon_id,
                team_lead.clone(),
            ))
        {
            return Err(HackathonError::AlreadyRegistered);
        }

        if hackathon.submission_count >= hackathon.max_participants {
            return Err(HackathonError::MaxParticipantsReached);
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
            &HackathonDataKey::Submission(hackathon_id, team_lead.clone()),
            &submission,
        );
        env.storage().persistent().set(
            &HackathonDataKey::SubmissionIndex(hackathon_id, idx),
            &team_lead.clone(),
        );

        hackathon.submission_count += 1;
        let hack_key = HackathonDataKey::Hackathon(hackathon_id);
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
    ) -> Result<(), HackathonError> {
        team_lead.require_auth();

        let hackathon = Self::load_hackathon(&env, hackathon_id)?;

        if env.ledger().timestamp() > hackathon.submission_deadline {
            return Err(HackathonError::SubmissionClosed);
        }

        let sub_key = HackathonDataKey::Submission(hackathon_id, team_lead.clone());
        let mut submission: Submission = env
            .storage()
            .persistent()
            .get(&sub_key)
            .ok_or(HackathonError::NotRegistered)?;

        if submission.submitted_at > 0 {
            return Err(HackathonError::AlreadySubmitted);
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

    pub fn open_judging(env: Env, hackathon_id: u64) -> Result<(), HackathonError> {
        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;

        if hackathon.status != HackathonStatus::Registration
            && hackathon.status != HackathonStatus::Submission
        {
            return Err(HackathonError::InvalidStatus);
        }

        if env.ledger().timestamp() <= hackathon.submission_deadline {
            return Err(HackathonError::SubmissionPeriodNotEnded);
        }

        hackathon.status = HackathonStatus::Judging;
        env.storage()
            .persistent()
            .set(&HackathonDataKey::Hackathon(hackathon_id), &hackathon);

        Ok(())
    }

    pub fn score_submission(
        env: Env,
        hackathon_id: u64,
        judge: Address,
        team_lead: Address,
        score: u32,
    ) -> Result<(), HackathonError> {
        judge.require_auth();

        let hackathon = Self::load_hackathon(&env, hackathon_id)?;

        if hackathon.status != HackathonStatus::Judging {
            return Err(HackathonError::JudgingNotActive);
        }
        if env.ledger().timestamp() > hackathon.judging_deadline {
            return Err(HackathonError::JudgingNotActive);
        }
        if score > 100 {
            return Err(HackathonError::InvalidScore);
        }

        if !env
            .storage()
            .persistent()
            .has(&HackathonDataKey::Judge(hackathon_id, judge.clone()))
        {
            return Err(HackathonError::NotAJudge);
        }

        let sub_key = HackathonDataKey::Submission(hackathon_id, team_lead.clone());
        let mut submission: Submission = env
            .storage()
            .persistent()
            .get(&sub_key)
            .ok_or(HackathonError::SubmissionNotFound)?;

        let score_key =
            HackathonDataKey::JudgeScore(hackathon_id, team_lead.clone(), judge.clone());
        if env.storage().persistent().has(&score_key) {
            return Err(HackathonError::AlreadyScored);
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

    /// Finalize a hackathon. Records winner ranks, amounts, and reputation,
    /// but does NOT transfer funds. Winners pull their prize via `claim_prize`
    /// within the claim window (default 90 days). After the window expires,
    /// the creator may sweep unclaimed amounts via `reclaim_unclaimed_prizes`.
    ///
    /// Permissionless: anyone can call this once `judging_deadline` has passed.
    pub fn finalize_hackathon(env: Env, hackathon_id: u64) -> Result<(), HackathonError> {
        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;

        if env.ledger().timestamp() <= hackathon.judging_deadline {
            return Err(HackathonError::JudgingNotOver);
        }

        if hackathon.status == HackathonStatus::Completed
            || hackathon.status == HackathonStatus::Cancelled
        {
            return Err(HackathonError::InvalidStatus);
        }

        let rep_addr = Self::get_rep_addr(&env)?;

        let sub_count = hackathon.submission_count;
        if sub_count == 0 {
            return Err(HackathonError::NoSubmissions);
        }

        // Gather submissions sorted by avg score (descending)
        let mut leads: Vec<Address> = Vec::new(&env);
        let mut scores: Vec<u32> = Vec::new(&env);

        for i in 0..sub_count {
            let lead: Address = env
                .storage()
                .persistent()
                .get(&HackathonDataKey::SubmissionIndex(hackathon_id, i))
                .unwrap();
            let sub: Submission = env
                .storage()
                .persistent()
                .get(&HackathonDataKey::Submission(hackathon_id, lead.clone()))
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
                .has(&HackathonDataKey::PrizeTier(hackathon_id, tier_count))
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
        let now = env.ledger().timestamp();
        let claim_window = Self::get_claim_window_internal(&env);
        let claim_deadline = now.saturating_add(claim_window);

        for rank in 0..num_winners {
            let lead = leads.get(rank).unwrap();
            let pct: u32 = env
                .storage()
                .persistent()
                .get(&HackathonDataKey::PrizeTier(hackathon_id, rank))
                .unwrap();

            let amount = hackathon
                .prize_pool
                .checked_mul(pct as i128)
                .ok_or(HackathonError::Overflow)?
                / 10000;

            // Record winner (no token transfer; winners pull via claim_prize)
            let record = WinnerRecord {
                hackathon_id,
                rank,
                winner: lead.clone(),
                amount,
                claimed: false,
                claimed_at: None,
                claim_deadline,
            };
            let winner_key = HackathonDataKey::Winner(hackathon_id, rank);
            env.storage().persistent().set(&winner_key, &record);
            Self::extend_persistent_ttl(&env, &winner_key);

            let by_addr_key = HackathonDataKey::WinnerByAddress(hackathon_id, lead.clone());
            env.storage().persistent().set(&by_addr_key, &rank);
            Self::extend_persistent_ttl(&env, &by_addr_key);

            // Record hackathon result in reputation (unchanged from previous behavior)
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

        env.storage()
            .persistent()
            .set(&HackathonDataKey::WinnerCount(hackathon_id), &num_winners);

        hackathon.status = HackathonStatus::Completed;
        env.storage()
            .persistent()
            .set(&HackathonDataKey::Hackathon(hackathon_id), &hackathon);

        HackathonFinalized {
            hackathon_id,
            winner_count: num_winners,
        }
        .publish(&env);

        Ok(())
    }

    /// Winner pulls their prize from the escrow pool.
    ///
    /// Auth: `winner.require_auth()` — works for both classic G-addresses and
    /// SAK smart-account contracts (via `__check_auth`).
    ///
    /// Idempotent: subsequent calls return `AlreadyClaimed`.
    /// Time-bound: after `claim_deadline`, returns `ClaimWindowExpired`.
    pub fn claim_prize(
        env: Env,
        hackathon_id: u64,
        winner: Address,
    ) -> Result<i128, HackathonError> {
        winner.require_auth();

        let hackathon = Self::load_hackathon(&env, hackathon_id)?;
        if hackathon.status != HackathonStatus::Completed {
            return Err(HackathonError::HackathonNotFinalized);
        }

        let rank: u32 = env
            .storage()
            .persistent()
            .get(&HackathonDataKey::WinnerByAddress(
                hackathon_id,
                winner.clone(),
            ))
            .ok_or(HackathonError::NotAWinner)?;

        let winner_key = HackathonDataKey::Winner(hackathon_id, rank);
        let mut record: WinnerRecord = env
            .storage()
            .persistent()
            .get(&winner_key)
            .ok_or(HackathonError::NotAWinner)?;

        if record.claimed {
            return Err(HackathonError::AlreadyClaimed);
        }
        if env.ledger().timestamp() > record.claim_deadline {
            return Err(HackathonError::ClaimWindowExpired);
        }

        // Mark claimed BEFORE the cross-contract call to prevent reentrancy.
        let amount = record.amount;
        record.claimed = true;
        record.claimed_at = Some(env.ledger().timestamp());
        env.storage().persistent().set(&winner_key, &record);

        // Release from escrow (only after state is updated)
        if amount > 0 {
            let escrow_addr = Self::get_escrow_addr(&env)?;
            let release_args: Vec<Val> = Vec::from_array(
                &env,
                [
                    hackathon.pool_id.clone().into_val(&env),
                    winner.clone().into_val(&env),
                    amount.into_val(&env),
                ],
            );
            env.invoke_contract::<()>(&escrow_addr, &sym(&env, "release_partial"), release_args);
        }

        PrizeClaimed {
            hackathon_id,
            winner,
            rank,
            amount,
        }
        .publish(&env);

        Self::extend_persistent_ttl(&env, &winner_key);
        Self::extend_instance_ttl(&env);

        Ok(amount)
    }

    /// After `claim_deadline` passes on a finalized hackathon, the creator
    /// may sweep all unclaimed winner amounts back to themselves. Idempotent —
    /// already-claimed and already-reclaimed records are skipped.
    pub fn reclaim_unclaimed_prizes(
        env: Env,
        hackathon_id: u64,
    ) -> Result<i128, HackathonError> {
        let hackathon = Self::load_hackathon(&env, hackathon_id)?;
        hackathon.creator.require_auth();

        if hackathon.status != HackathonStatus::Completed {
            return Err(HackathonError::HackathonNotFinalized);
        }

        let winner_count: u32 = env
            .storage()
            .persistent()
            .get(&HackathonDataKey::WinnerCount(hackathon_id))
            .unwrap_or(0);

        if winner_count == 0 {
            return Ok(0);
        }

        let now = env.ledger().timestamp();
        let escrow_addr = Self::get_escrow_addr(&env)?;
        let mut total_reclaimed: i128 = 0;
        let mut any_eligible = false;

        for rank in 0..winner_count {
            let winner_key = HackathonDataKey::Winner(hackathon_id, rank);
            let mut record: WinnerRecord = env
                .storage()
                .persistent()
                .get(&winner_key)
                .ok_or(HackathonError::NotAWinner)?;

            if record.claimed {
                continue;
            }
            if now <= record.claim_deadline {
                // Window still open for this record; not yet reclaimable.
                continue;
            }
            any_eligible = true;

            // Mark claimed BEFORE releasing to prevent re-reclaim/reentrancy.
            let amount = record.amount;
            record.claimed = true;
            record.claimed_at = Some(now);
            env.storage().persistent().set(&winner_key, &record);

            if amount > 0 {
                let release_args: Vec<Val> = Vec::from_array(
                    &env,
                    [
                        hackathon.pool_id.clone().into_val(&env),
                        hackathon.creator.clone().into_val(&env),
                        amount.into_val(&env),
                    ],
                );
                env.invoke_contract::<()>(
                    &escrow_addr,
                    &sym(&env, "release_partial"),
                    release_args,
                );
                total_reclaimed = total_reclaimed
                    .checked_add(amount)
                    .ok_or(HackathonError::Overflow)?;
            }
        }

        // If the caller invokes this before ANY record has expired, surface an
        // explicit error so they don't silently get back 0.
        if !any_eligible {
            return Err(HackathonError::ClaimWindowNotExpired);
        }

        if total_reclaimed > 0 {
            UnclaimedPrizesReclaimed {
                hackathon_id,
                total_amount: total_reclaimed,
            }
            .publish(&env);
        }

        Self::extend_instance_ttl(&env);

        Ok(total_reclaimed)
    }

    /// Admin-only. Configure the claim window in seconds. Affects hackathons
    /// finalized AFTER this call. Existing winner records keep their original
    /// `claim_deadline`. Recommended values: 30-180 days.
    pub fn set_claim_window(env: Env, window_seconds: u64) -> Result<(), HackathonError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&HackathonDataKey::Admin)
            .ok_or(HackathonError::NotInitialized)?;
        admin.require_auth();

        // Guardrail: must be at least 1 day, at most 1 year.
        if window_seconds < 86_400 || window_seconds > 365 * 86_400 {
            return Err(HackathonError::InvalidClaimWindow);
        }

        env.storage()
            .instance()
            .set(&HackathonDataKey::ClaimWindow, &window_seconds);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    /// Returns the currently configured claim window in seconds.
    /// Falls back to `DEFAULT_CLAIM_WINDOW_SECONDS` if admin has not set one.
    pub fn get_claim_window(env: Env) -> u64 {
        Self::get_claim_window_internal(&env)
    }

    // ========================================================================
    // ADMIN
    // ========================================================================

    pub fn disqualify_submission(
        env: Env,
        hackathon_id: u64,
        team_lead: Address,
    ) -> Result<(), HackathonError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&HackathonDataKey::Admin)
            .ok_or(HackathonError::NotInitialized)?;
        admin.require_auth();

        let sub_key = HackathonDataKey::Submission(hackathon_id, team_lead);
        let mut submission: Submission = env
            .storage()
            .persistent()
            .get(&sub_key)
            .ok_or(HackathonError::SubmissionNotFound)?;

        if submission.disqualified {
            return Err(HackathonError::AlreadyDisqualified);
        }

        submission.disqualified = true;
        env.storage().persistent().set(&sub_key, &submission);

        Ok(())
    }

    pub fn cancel_hackathon(env: Env, hackathon_id: u64) -> Result<(), HackathonError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&HackathonDataKey::Admin)
            .ok_or(HackathonError::NotInitialized)?;
        admin.require_auth();

        let mut hackathon = Self::load_hackathon(&env, hackathon_id)?;

        if hackathon.status == HackathonStatus::Completed
            || hackathon.status == HackathonStatus::Cancelled
        {
            return Err(HackathonError::HackathonNotCancellable);
        }

        let escrow_addr = Self::get_escrow_addr(&env)?;
        let refund_args: Vec<Val> =
            Vec::from_array(&env, [hackathon.pool_id.clone().into_val(&env)]);
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "refund_all"), refund_args);

        hackathon.status = HackathonStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&HackathonDataKey::Hackathon(hackathon_id), &hackathon);

        HackathonCancelled { hackathon_id }.publish(&env);

        Ok(())
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), HackathonError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&HackathonDataKey::Admin)
            .ok_or(HackathonError::NotInitialized)?;
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
    ) -> Result<u32, HackathonError> {
        sponsor.require_auth();

        let hackathon = Self::load_hackathon(&env, hackathon_id)?;

        if hackathon.status != HackathonStatus::Registration
            && hackathon.status != HackathonStatus::Submission
        {
            return Err(HackathonError::InvalidTrackStatus);
        }

        let track_count_key = HackathonDataKey::HackathonTrackCount(hackathon_id);
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

        let track_key = HackathonDataKey::HackathonTrack(hackathon_id, track_id);
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
    ) -> Result<(), HackathonError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&HackathonDataKey::Admin)
            .ok_or(HackathonError::NotInitialized)?;
        admin.require_auth();

        let hackathon = Self::load_hackathon(&env, hackathon_id)?;

        if hackathon.status != HackathonStatus::Judging
            && hackathon.status != HackathonStatus::Completed
        {
            return Err(HackathonError::InvalidStatus);
        }

        let track_key = HackathonDataKey::HackathonTrack(hackathon_id, track_id);
        let track: SponsoredTrack = env
            .storage()
            .persistent()
            .get(&track_key)
            .ok_or(HackathonError::TrackNotFound)?;

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

    pub fn get_hackathon(env: Env, id: u64) -> Result<Hackathon, HackathonError> {
        let key = HackathonDataKey::Hackathon(id);
        let hackathon = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(HackathonError::HackathonNotFound)?;
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);
        Ok(hackathon)
    }

    pub fn get_submission(
        env: Env,
        hackathon_id: u64,
        team_lead: Address,
    ) -> Result<Submission, HackathonError> {
        env.storage()
            .persistent()
            .get(&HackathonDataKey::Submission(hackathon_id, team_lead))
            .ok_or(HackathonError::SubmissionNotFound)
    }

    /// Look up a winner record by hackathon + rank. Returns `NotAWinner` if
    /// no record exists at that rank.
    pub fn get_winner(
        env: Env,
        hackathon_id: u64,
        rank: u32,
    ) -> Result<WinnerRecord, HackathonError> {
        env.storage()
            .persistent()
            .get(&HackathonDataKey::Winner(hackathon_id, rank))
            .ok_or(HackathonError::NotAWinner)
    }

    /// Look up a winner record by hackathon + address. Returns `NotAWinner` if
    /// the address is not in the winner set.
    pub fn get_winner_by_address(
        env: Env,
        hackathon_id: u64,
        winner: Address,
    ) -> Result<WinnerRecord, HackathonError> {
        let rank: u32 = env
            .storage()
            .persistent()
            .get(&HackathonDataKey::WinnerByAddress(hackathon_id, winner))
            .ok_or(HackathonError::NotAWinner)?;
        env.storage()
            .persistent()
            .get(&HackathonDataKey::Winner(hackathon_id, rank))
            .ok_or(HackathonError::NotAWinner)
    }

    /// Returns the number of winners recorded for a finalized hackathon.
    /// Returns 0 for hackathons that are not yet finalized or had no submissions.
    pub fn get_winner_count(env: Env, hackathon_id: u64) -> u32 {
        env.storage()
            .persistent()
            .get(&HackathonDataKey::WinnerCount(hackathon_id))
            .unwrap_or(0)
    }

    // ========================================================================
    // INTERNAL
    // ========================================================================

    fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND);
    }

    fn extend_persistent_ttl(env: &Env, key: &HackathonDataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND);
    }

    fn load_hackathon(env: &Env, id: u64) -> Result<Hackathon, HackathonError> {
        env.storage()
            .persistent()
            .get(&HackathonDataKey::Hackathon(id))
            .ok_or(HackathonError::HackathonNotFound)
    }

    fn get_escrow_addr(env: &Env) -> Result<Address, HackathonError> {
        env.storage()
            .instance()
            .get(&HackathonDataKey::CoreEscrow)
            .ok_or(HackathonError::NotInitialized)
    }

    fn get_rep_addr(env: &Env) -> Result<Address, HackathonError> {
        env.storage()
            .instance()
            .get(&HackathonDataKey::ReputationRegistry)
            .ok_or(HackathonError::NotInitialized)
    }

    fn get_claim_window_internal(env: &Env) -> u64 {
        env.storage()
            .instance()
            .get(&HackathonDataKey::ClaimWindow)
            .unwrap_or(DEFAULT_CLAIM_WINDOW_SECONDS)
    }
}
