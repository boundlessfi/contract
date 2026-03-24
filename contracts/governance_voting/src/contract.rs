use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

use boundless_types::math::int_sqrt_i128;
use boundless_types::ttl::{
    INSTANCE_TTL_EXTEND, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD,
};

use crate::error::Error;
use crate::events::{QFDonationRecorded, SessionConcluded, SessionCreated, VoteCast};
use crate::storage::{
    DataKey, VoteContext, VoteOption, VoteRecord, VoteStatus, VotingSession,
};

#[contract]
pub struct GovernanceVoting;

#[contractimpl]
impl GovernanceVoting {
    // ========================================================================
    // INITIALIZATION
    // ========================================================================

    pub fn init(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    // ========================================================================
    // MODULE AUTHORIZATION
    // ========================================================================

    pub fn add_authorized_module(env: Env, module: Address) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::AuthorizedModule(module), &true);
        Ok(())
    }

    pub fn remove_authorized_module(env: Env, module: Address) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.storage()
            .instance()
            .remove(&DataKey::AuthorizedModule(module));
        Ok(())
    }

    // ========================================================================
    // SESSION MANAGEMENT
    // ========================================================================

    pub fn create_session(
        env: Env,
        module: Address,
        context: VoteContext,
        module_id: u64,
        options: Vec<String>,
        start_at: u64,
        end_at: u64,
        threshold: Option<u32>,
        quorum: Option<u32>,
        weight_by_reputation: bool,
    ) -> Result<BytesN<32>, Error> {
        module.require_auth();
        if !Self::is_module_authorized_internal(&env, &module) {
            return Err(Error::ModuleNotAuthorized);
        }
        if start_at >= end_at {
            return Err(Error::InvalidTimeRange);
        }

        // session_id = sha256(context_byte ++ module_id_bytes)
        let context_byte: u8 = match context {
            VoteContext::CampaignValidation => 0,
            VoteContext::RetrospectiveGrant => 1,
            VoteContext::QFRound => 2,
            VoteContext::HackathonJudging => 3,
        };
        let mut payload = soroban_sdk::Bytes::new(&env);
        payload.push_back(context_byte);
        let id_bytes = module_id.to_be_bytes();
        for b in id_bytes {
            payload.push_back(b);
        }
        let session_id: BytesN<32> = env.crypto().sha256(&payload).into();

        let option_count = options.len();
        for i in 0..option_count {
            let label = options.get(i).ok_or(Error::InvalidOption)?;
            let opt = VoteOption {
                id: i,
                label,
                votes: 0,
                weighted_votes: 0,
            };
            env.storage()
                .persistent()
                .set(&DataKey::VoteOption(session_id.clone(), i), &opt);
        }
        env.storage()
            .persistent()
            .set(&DataKey::OptionCount(session_id.clone()), &option_count);

        let session = VotingSession {
            session_id: session_id.clone(),
            context: context.clone(),
            module_id,
            created_at: env.ledger().timestamp(),
            start_at,
            end_at,
            status: VoteStatus::Active,
            threshold,
            threshold_reached: false,
            total_votes: 0,
            quorum,
            weight_by_reputation,
        };

        let session_key = DataKey::Session(session_id.clone());
        env.storage()
            .persistent()
            .set(&session_key, &session);
        Self::extend_persistent_ttl(&env, &session_key);
        Self::extend_instance_ttl(&env);

        SessionCreated {
            session_id: session_id.clone(),
            context,
            module_id,
        }
        .publish(&env);

        Ok(session_id)
    }

    // ========================================================================
    // VOTING
    // ========================================================================

    pub fn cast_vote(
        env: Env,
        voter: Address,
        session_id: BytesN<32>,
        option_id: u32,
    ) -> Result<(), Error> {
        voter.require_auth();

        let mut session: VotingSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id.clone()))
            .ok_or(Error::SessionNotFound)?;

        if session.status != VoteStatus::Active {
            return Err(Error::SessionNotActive);
        }
        let now = env.ledger().timestamp();
        if now < session.start_at {
            return Err(Error::VotingNotStarted);
        }
        if now > session.end_at {
            return Err(Error::SessionNotActive);
        }

        let vote_key = DataKey::VoteRecord(session_id.clone(), voter.clone());
        if env.storage().persistent().has(&vote_key) {
            return Err(Error::AlreadyVoted);
        }

        // Validate option exists
        let opt_key = DataKey::VoteOption(session_id.clone(), option_id);
        let mut option: VoteOption = env
            .storage()
            .persistent()
            .get(&opt_key)
            .ok_or(Error::InvalidOption)?;

        let weight: u32 = if session.weight_by_reputation {
            // placeholder: actual weight fetched by caller
            1
        } else {
            1
        };

        option.votes = option.votes.saturating_add(1);
        option.weighted_votes = option.weighted_votes.saturating_add(weight as u64);
        env.storage().persistent().set(&opt_key, &option);

        session.total_votes = session.total_votes.saturating_add(1);
        if let Some(threshold) = session.threshold {
            if session.total_votes >= threshold {
                session.threshold_reached = true;
            }
        }
        let sess_key = DataKey::Session(session_id.clone());
        env.storage()
            .persistent()
            .set(&sess_key, &session);
        Self::extend_persistent_ttl(&env, &sess_key);
        Self::extend_instance_ttl(&env);

        let record = VoteRecord {
            voter: voter.clone(),
            option_id,
            weight,
            voted_at: now,
        };
        env.storage().persistent().set(&vote_key, &record);

        VoteCast {
            session_id,
            voter,
            option_id,
        }
        .publish(&env);

        Ok(())
    }

    // ========================================================================
    // SESSION LIFECYCLE
    // ========================================================================

    pub fn conclude_session(env: Env, session_id: BytesN<32>) -> Result<(), Error> {
        let mut session: VotingSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id.clone()))
            .ok_or(Error::SessionNotFound)?;

        if env.ledger().timestamp() <= session.end_at {
            return Err(Error::SessionNotEnded);
        }

        session.status = VoteStatus::Concluded;
        env.storage()
            .persistent()
            .set(&DataKey::Session(session_id.clone()), &session);

        SessionConcluded {
            session_id,
        }
        .publish(&env);

        Ok(())
    }

    pub fn cancel_session(env: Env, session_id: BytesN<32>) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let mut session: VotingSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id.clone()))
            .ok_or(Error::SessionNotFound)?;

        session.status = VoteStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Session(session_id.clone()), &session);

        Ok(())
    }

    // ========================================================================
    // QF FUNCTIONS
    // ========================================================================

    pub fn record_qf_donation(
        env: Env,
        session_id: BytesN<32>,
        module: Address,
        amount: i128,
        option_id: u32,
    ) -> Result<(), Error> {
        module.require_auth();
        if !Self::is_module_authorized_internal(&env, &module) {
            return Err(Error::ModuleNotAuthorized);
        }

        let session: VotingSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id.clone()))
            .ok_or(Error::SessionNotFound)?;

        if session.status != VoteStatus::Active {
            return Err(Error::SessionNotActive);
        }

        // Validate option exists
        let option_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::OptionCount(session_id.clone()))
            .unwrap_or(0);
        if option_id >= option_count {
            return Err(Error::InvalidOption);
        }

        // Maintain running sum-of-sqrt per option for QF distribution
        let sum_sqrt_key = DataKey::OptionSumSqrt(session_id.clone(), option_id);
        let old_sum_sqrt: i128 = env.storage().persistent().get(&sum_sqrt_key).unwrap_or(0);

        // We don't track per-donor here for simplicity — each call is a new donation
        let scaled_amount = amount.checked_mul(1_000_000).ok_or(Error::Overflow)?;
        let sqrt_val = int_sqrt_i128(scaled_amount).ok_or(Error::InvalidOption)?;
        let new_sum_sqrt = old_sum_sqrt.checked_add(sqrt_val).ok_or(Error::Overflow)?;
        env.storage().persistent().set(&sum_sqrt_key, &new_sum_sqrt);

        QFDonationRecorded {
            session_id,
            donor: module.clone(),
            option_id,
            amount,
        }
        .publish(&env);

        Ok(())
    }

    pub fn compute_qf_distribution(
        env: Env,
        session_id: BytesN<32>,
        matching_pool: i128,
    ) -> Result<Vec<(u32, i128)>, Error> {
        let session: VotingSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id.clone()))
            .ok_or(Error::SessionNotFound)?;

        if env.ledger().timestamp() <= session.end_at {
            return Err(Error::SessionNotEnded);
        }

        let option_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::OptionCount(session_id.clone()))
            .unwrap_or(0);

        // Use per-option sum_sqrt maintained by record_qf_donation
        let mut results = Vec::new(&env);
        let mut squares = Vec::new(&env);
        let mut total_squares: i128 = 0;

        for i in 0..option_count {
            let sum_sqrt_key = DataKey::OptionSumSqrt(session_id.clone(), i);
            let sum_sqrt: i128 = env.storage().persistent().get(&sum_sqrt_key).unwrap_or(0);
            let sq = sum_sqrt.checked_mul(sum_sqrt).ok_or(Error::Overflow)?;
            squares.push_back(sq);
            total_squares = total_squares.checked_add(sq).ok_or(Error::Overflow)?;
        }

        if total_squares == 0 {
            for i in 0..option_count {
                results.push_back((i, 0i128));
            }
            return Ok(results);
        }

        for i in 0..option_count {
            let sq = squares.get(i).ok_or(Error::InvalidOption)?;
            let share = sq.checked_mul(matching_pool).ok_or(Error::Overflow)?
                .checked_div(total_squares).ok_or(Error::Overflow)?;
            results.push_back((i, share));
        }

        Ok(results)
    }

    // ========================================================================
    // QUERY FUNCTIONS
    // ========================================================================

    pub fn get_session(env: Env, session_id: BytesN<32>) -> Result<VotingSession, Error> {
        let key = DataKey::Session(session_id);
        let session = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::SessionNotFound)?;
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);
        Ok(session)
    }

    pub fn get_result(env: Env, session_id: BytesN<32>) -> Result<Vec<VoteOption>, Error> {
        let _session: VotingSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id.clone()))
            .ok_or(Error::SessionNotFound)?;

        let option_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::OptionCount(session_id.clone()))
            .unwrap_or(0);

        let mut results = Vec::new(&env);
        for i in 0..option_count {
            let opt: VoteOption = env
                .storage()
                .persistent()
                .get(&DataKey::VoteOption(session_id.clone(), i))
                .ok_or(Error::InvalidOption)?;
            results.push_back(opt);
        }
        Ok(results)
    }

    pub fn get_option(env: Env, session_id: BytesN<32>, option_id: u32) -> Result<VoteOption, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::VoteOption(session_id, option_id))
            .ok_or(Error::InvalidOption)
    }

    pub fn has_voted(env: Env, session_id: BytesN<32>, voter: Address) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::VoteRecord(session_id, voter))
    }

    pub fn threshold_reached(env: Env, session_id: BytesN<32>) -> Result<bool, Error> {
        let session: VotingSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id))
            .ok_or(Error::SessionNotFound)?;
        Ok(session.threshold_reached)
    }

    // ========================================================================
    // INTERNAL HELPERS
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

    fn is_module_authorized_internal(env: &Env, module: &Address) -> bool {
        // Admin is always authorized
        let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
        if let Some(a) = admin {
            if module == &a {
                return true;
            }
        }
        env.storage()
            .instance()
            .get(&DataKey::AuthorizedModule(module.clone()))
            .unwrap_or(false)
    }
}
