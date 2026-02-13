use soroban_sdk::{contract, contractimpl, xdr::ToXdr, Address, BytesN, Env, String, Vec};

use crate::error::Error;
use crate::events::{ModuleAuthorized, QFDonationRecorded, SessionCreated, VoteCast};
use crate::storage::{DataKey, VoteContext, VoteOption, VoteRecord, VoteStatus, VotingSession};

#[contract]
pub struct GovernanceVoting;

#[contractimpl]
impl GovernanceVoting {
    pub fn init_gov_voting(env: Env, admin: Address, reputation_registry: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::ReputationRegistry, &reputation_registry);
        Ok(())
    }

    pub fn add_gov_module(env: Env, module: Address) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::AuthorizedModule(module.clone()), &true);
        ModuleAuthorized {
            module,
            authorized: true,
        }
        .publish(&env);
        Ok(())
    }

    pub fn remove_gov_module(env: Env, module: Address) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.storage()
            .instance()
            .remove(&DataKey::AuthorizedModule(module.clone()));
        ModuleAuthorized {
            module,
            authorized: false,
        }
        .publish(&env);
        Ok(())
    }

    fn is_authorized(env: &Env, module: Address) -> bool {
        let admin: Address = match env.storage().instance().get(&DataKey::Admin) {
            Some(a) => a,
            None => return false,
        };
        if module == admin {
            return true;
        }
        env.storage()
            .instance()
            .get(&DataKey::AuthorizedModule(module))
            .unwrap_or(false)
    }

    pub fn create_session(
        env: Env,
        module: Address,
        context: VoteContext,
        module_id: u64,
        options_labels: Vec<String>,
        start_at: u64,
        end_at: u64,
        threshold: Option<u32>,
        weight_by_reputation: bool,
    ) -> Result<BytesN<32>, Error> {
        module.require_auth();
        if !Self::is_authorized(&env, module.clone()) {
            return Err(Error::NotAuthorized);
        }

        let mut data = Vec::new(&env);
        data.push_back(module_id);
        data.push_back(start_at);
        let session_id: BytesN<32> = env.crypto().sha256(&data.to_xdr(&env)).into();

        if env
            .storage()
            .persistent()
            .has(&DataKey::Session(session_id.clone()))
        {
            return Err(Error::SessionCollision);
        }

        let option_count = options_labels.len();
        for (i, label) in options_labels.iter().enumerate() {
            let opt = VoteOption {
                id: i as u32,
                label: label.clone(),
                votes: 0,
                weighted_votes: 0,
            };
            env.storage()
                .persistent()
                .set(&DataKey::Option(session_id.clone(), i as u32), &opt);
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
            quorum: None,
            weight_by_reputation,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Session(session_id.clone()), &session);

        SessionCreated {
            session_id: session_id.clone(),
            context,
            module_id,
        }
        .publish(&env);

        Ok(session_id)
    }

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
        if env.ledger().timestamp() < session.start_at {
            return Err(Error::VotingNotStarted);
        }
        if env.ledger().timestamp() > session.end_at {
            return Err(Error::VotingEnded);
        }

        let vote_key = DataKey::Vote(session_id.clone(), voter.clone());
        if env.storage().persistent().has(&vote_key) {
            return Err(Error::AlreadyVoted);
        }

        let mut weight = 1u32;
        if session.weight_by_reputation {
            let rep_addr: Address = env
                .storage()
                .instance()
                .get(&DataKey::ReputationRegistry)
                .ok_or(Error::NotInitialized)?;
            let rep_client = reputation_registry::ReputationRegistryClient::new(&env, &rep_addr);
            let profile = rep_client.get_reputation(&voter);
            weight = profile.level + 1;
        }

        let opt_key = DataKey::Option(session_id.clone(), option_id);
        let mut option: VoteOption = env
            .storage()
            .persistent()
            .get(&opt_key)
            .ok_or(Error::OptionNotFound)?;

        option.votes += 1;
        option.weighted_votes += weight as i128;
        env.storage().persistent().set(&opt_key, &option);

        session.total_votes += 1;
        if let Some(threshold) = session.threshold {
            if session.total_votes >= threshold {
                session.threshold_reached = true;
            }
        }
        env.storage()
            .persistent()
            .set(&DataKey::Session(session_id.clone()), &session);

        let record = VoteRecord {
            voter: voter.clone(),
            option_id,
            weight,
            timestamp: env.ledger().timestamp(),
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

    pub fn get_winning_option(env: Env, session_id: BytesN<32>) -> Result<u32, Error> {
        let session: VotingSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id.clone()))
            .ok_or(Error::SessionNotFound)?;

        if session.status == VoteStatus::Active && env.ledger().timestamp() < session.end_at {
            return Err(Error::VotingInProgress);
        }

        let option_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::OptionCount(session_id.clone()))
            .unwrap_or(0);
        let mut max_votes = -1i128;
        let mut winner = 0u32;

        for i in 0..option_count {
            let opt: VoteOption = env
                .storage()
                .persistent()
                .get(&DataKey::Option(session_id.clone(), i))
                .ok_or(Error::OptionNotFound)?;
            if opt.weighted_votes > max_votes {
                max_votes = opt.weighted_votes;
                winner = i;
            }
        }
        Ok(winner)
    }

    // For QF: Record donation
    pub fn record_qf_donation(
        env: Env,
        caller: Address,
        session_id: BytesN<32>,
        donor: Address,
        option_id: u32,
        amount: i128,
    ) -> Result<(), Error> {
        caller.require_auth();
        if !Self::is_authorized(&env, caller.clone()) {
            return Err(Error::NotAuthorized);
        }

        let session: VotingSession = env
            .storage()
            .persistent()
            .get(&DataKey::Session(session_id.clone()))
            .ok_or(Error::SessionNotFound)?;

        if session.context != VoteContext::QFRound {
            return Err(Error::NotQFRound);
        }

        let sqrt_amt = int_sqrt(amount);
        let sum_key = DataKey::OptionSumSqrt(session_id.clone(), option_id);
        let current_sum: i128 = env.storage().persistent().get(&sum_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&sum_key, &(current_sum + sqrt_amt));

        QFDonationRecorded {
            session_id,
            donor,
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
            return Err(Error::RoundNotEnded);
        }

        let option_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::OptionCount(session_id.clone()))
            .unwrap_or(0);
        let mut results = Vec::new(&env);
        let mut squares = Vec::new(&env);
        let mut total_squares = 0i128;

        for i in 0..option_count {
            let sum_sqrt: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::OptionSumSqrt(session_id.clone(), i))
                .unwrap_or(0);
            let sq = sum_sqrt * sum_sqrt;
            squares.push_back(sq);
            total_squares += sq;
        }

        if total_squares == 0 {
            return Ok(results);
        }

        for i in 0..option_count {
            let sq = squares.get(i).unwrap();
            let share = (sq * matching_pool) / total_squares;
            results.push_back((i, share));
        }
        Ok(results)
    }

    pub fn get_session(env: Env, session_id: BytesN<32>) -> Result<VotingSession, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Session(session_id))
            .ok_or(Error::SessionNotFound)
    }

    pub fn get_option(
        env: Env,
        session_id: BytesN<32>,
        option_id: u32,
    ) -> Result<VoteOption, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Option(session_id, option_id))
            .ok_or(Error::OptionNotFound)
    }
}

// Helper sqrt
fn int_sqrt(n: i128) -> i128 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}
