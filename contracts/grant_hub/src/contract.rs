use soroban_sdk::{
    contract, contractimpl, Address, BytesN, Env, IntoVal, String, Symbol, Val, Vec,
};

use boundless_types::ttl::{
    INSTANCE_TTL_EXTEND, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD,
};
use boundless_types::ModuleType;

use crate::error::Error;
use crate::events::{
    GrantCompleted, GrantCreated, MilestoneApproved, MilestoneSubmitted, QFDonationMade,
};
use crate::storage::{
    DataKey, Grant, GrantMilestone, GrantStatus, GrantType, MilestoneStatus, QFRoundData,
    VoteContext, VoteOption,
};

// Reusable symbols for cross-contract calls (avoids importing full client ABIs)
fn sym(env: &Env, name: &str) -> Symbol {
    Symbol::new(env, name)
}

#[contract]
pub struct GrantHub;

#[contractimpl]
impl GrantHub {
    // ========================================================================
    // INITIALIZATION
    // ========================================================================

    pub fn init(
        env: Env,
        admin: Address,
        core_escrow: Address,
        reputation_registry: Address,
        governance_voting: Address,
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
            .set(&DataKey::GovernanceVoting, &governance_voting);
        env.storage().instance().set(&DataKey::GrantCount, &0u64);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    // ========================================================================
    // QUERIES
    // ========================================================================

    pub fn get_grant(env: Env, grant_id: u64) -> Result<Grant, Error> {
        let key = DataKey::Grant(grant_id);
        let grant = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::GrantNotFound)?;
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);
        Ok(grant)
    }

    pub fn get_milestone(
        env: Env,
        grant_id: u64,
        milestone_index: u32,
    ) -> Result<GrantMilestone, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::GrantMilestone(grant_id, milestone_index))
            .ok_or(Error::MilestoneNotFound)
    }

    pub fn get_qf_round(env: Env, grant_id: u64) -> Result<QFRoundData, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::QFRound(grant_id))
            .ok_or(Error::GrantNotFound)
    }

    pub fn get_retro_session(env: Env, grant_id: u64) -> Result<BytesN<32>, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::RetroSession(grant_id))
            .ok_or(Error::NoVoteSession)
    }

    // ========================================================================
    // MILESTONE GRANT FLOW
    // ========================================================================

    pub fn create_milestone_grant(
        env: Env,
        creator: Address,
        recipient: Address,
        amount: i128,
        asset: Address,
        milestone_descs: Vec<(String, u32)>,
    ) -> Result<u64, Error> {
        creator.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Validate milestones sum to 10000
        let mut total_pct: u32 = 0;
        for (_, pct) in milestone_descs.iter() {
            total_pct += pct;
        }
        if total_pct != 10000 {
            return Err(Error::InvalidMilestonePercents);
        }

        let count = Self::next_grant_id(&env);
        let escrow_addr = Self::get_escrow_addr(&env);

        let args: Vec<Val> = Vec::from_array(
            &env,
            [
                creator.into_val(&env),
                ModuleType::Grant.into_val(&env),
                count.into_val(&env),
                amount.into_val(&env),
                asset.clone().into_val(&env),
                (env.ledger().timestamp() + 31_536_000).into_val(&env),
                env.current_contract_address().into_val(&env),
            ],
        );
        let pool_id: BytesN<32> =
            env.invoke_contract(&escrow_addr, &sym(&env, "create_pool"), args);

        // Define release slots
        let mut slots: Vec<(Address, i128)> = Vec::new(&env);
        let milestone_count = milestone_descs.len();
        for (_, pct) in milestone_descs.iter() {
            let slot_amount = amount.checked_mul(pct as i128).ok_or(Error::Overflow)? / 10000;
            slots.push_back((recipient.clone(), slot_amount));
        }

        let slot_args: Vec<Val> =
            Vec::from_array(&env, [pool_id.clone().into_val(&env), slots.into_val(&env)]);
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "define_release_slots"), slot_args);

        let lock_args: Vec<Val> = Vec::from_array(&env, [pool_id.clone().into_val(&env)]);
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "lock_pool"), lock_args);

        // Store milestones decomposed
        for (i, (desc, pct)) in milestone_descs.iter().enumerate() {
            let milestone = GrantMilestone {
                id: i as u32,
                description: desc,
                pct,
                status: MilestoneStatus::Pending,
            };
            env.storage()
                .persistent()
                .set(&DataKey::GrantMilestone(count, i as u32), &milestone);
        }

        // Store recipient for later use when all milestones complete
        env.storage()
            .persistent()
            .set(&DataKey::GrantRecipient(count), &recipient);

        let grant = Grant {
            id: count,
            creator: creator.clone(),
            grant_type: GrantType::Milestone,
            status: GrantStatus::Active,
            amount,
            asset,
            pool_id,
            milestone_count,
            metadata_cid: String::from_str(&env, ""),
            created_at: env.ledger().timestamp(),
        };

        let grant_key = DataKey::Grant(count);
        env.storage().persistent().set(&grant_key, &grant);
        Self::extend_persistent_ttl(&env, &grant_key);
        Self::extend_instance_ttl(&env);

        GrantCreated {
            id: count,
            grant_type: GrantType::Milestone,
            creator,
            amount,
        }
        .publish(&env);

        Ok(count)
    }

    pub fn submit_grant_milestone(
        env: Env,
        recipient: Address,
        grant_id: u64,
        milestone_index: u32,
    ) -> Result<(), Error> {
        recipient.require_auth();

        let grant: Grant = env
            .storage()
            .persistent()
            .get(&DataKey::Grant(grant_id))
            .ok_or(Error::GrantNotFound)?;

        if grant.grant_type != GrantType::Milestone {
            return Err(Error::NotMilestoneGrant);
        }

        let key = DataKey::GrantMilestone(grant_id, milestone_index);
        let mut milestone: GrantMilestone = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::MilestoneNotFound)?;

        if milestone.status != MilestoneStatus::Pending
            && milestone.status != MilestoneStatus::Rejected
        {
            return Err(Error::InvalidMilestoneStatus);
        }

        milestone.status = MilestoneStatus::Submitted;
        env.storage().persistent().set(&key, &milestone);

        MilestoneSubmitted {
            grant_id,
            milestone_index,
        }
        .publish(&env);

        Ok(())
    }

    pub fn approve_grant_milestone(
        env: Env,
        grant_id: u64,
        milestone_index: u32,
    ) -> Result<(), Error> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let mut grant: Grant = env
            .storage()
            .persistent()
            .get(&DataKey::Grant(grant_id))
            .ok_or(Error::GrantNotFound)?;

        let key = DataKey::GrantMilestone(grant_id, milestone_index);
        let mut milestone: GrantMilestone = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::MilestoneNotFound)?;

        if milestone.status != MilestoneStatus::Submitted {
            return Err(Error::MilestoneNotSubmitted);
        }

        milestone.status = MilestoneStatus::Released;
        env.storage().persistent().set(&key, &milestone);

        // Release escrow slot
        let escrow_addr = Self::get_escrow_addr(&env);
        let args: Vec<Val> = Vec::from_array(
            &env,
            [
                grant.pool_id.clone().into_val(&env),
                milestone_index.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "release_slot"), args);

        MilestoneApproved {
            grant_id,
            milestone_index,
        }
        .publish(&env);

        // Check if all milestones are released
        let mut all_done = true;
        for i in 0..grant.milestone_count {
            if i == milestone_index {
                continue;
            }
            let m: GrantMilestone = env
                .storage()
                .persistent()
                .get(&DataKey::GrantMilestone(grant_id, i))
                .ok_or(Error::MilestoneNotFound)?;
            if m.status != MilestoneStatus::Released {
                all_done = false;
                break;
            }
        }

        if all_done {
            grant.status = GrantStatus::Completed;
            env.storage()
                .persistent()
                .set(&DataKey::Grant(grant_id), &grant);

            // Get recipient from stored key
            let recipient: Address = env
                .storage()
                .persistent()
                .get(&DataKey::GrantRecipient(grant_id))
                .ok_or(Error::GrantNotFound)?;

            // Record reputation
            let rep_addr = Self::get_rep_addr(&env);
            let rep_args: Vec<Val> = Vec::from_array(
                &env,
                [
                    env.current_contract_address().into_val(&env),
                    recipient.into_val(&env),
                    grant.amount.into_val(&env),
                ],
            );
            env.invoke_contract::<()>(&rep_addr, &sym(&env, "record_grant_received"), rep_args);

            GrantCompleted { grant_id }.publish(&env);
        } else if grant.status == GrantStatus::Active {
            grant.status = GrantStatus::Executing;
            env.storage()
                .persistent()
                .set(&DataKey::Grant(grant_id), &grant);
        }

        Ok(())
    }

    // ========================================================================
    // RETROSPECTIVE GRANT FLOW
    // ========================================================================

    pub fn create_retrospective_grant(
        env: Env,
        creator: Address,
        amount: i128,
        asset: Address,
        options: Vec<String>,
        voting_duration: u64,
    ) -> Result<u64, Error> {
        creator.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let count = Self::next_grant_id(&env);
        let escrow_addr = Self::get_escrow_addr(&env);

        let pool_args: Vec<Val> = Vec::from_array(
            &env,
            [
                creator.clone().into_val(&env),
                ModuleType::Grant.into_val(&env),
                count.into_val(&env),
                amount.into_val(&env),
                asset.clone().into_val(&env),
                (env.ledger().timestamp() + 31_536_000).into_val(&env),
                env.current_contract_address().into_val(&env),
            ],
        );
        let pool_id: BytesN<32> =
            env.invoke_contract(&escrow_addr, &sym(&env, "create_pool"), pool_args);

        let lock_args: Vec<Val> = Vec::from_array(&env, [pool_id.clone().into_val(&env)]);
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "lock_pool"), lock_args);

        // Create governance voting session
        let gov_addr = Self::get_gov_addr(&env);
        let now = env.ledger().timestamp();
        let gov_args: Vec<Val> = Vec::from_array(
            &env,
            [
                env.current_contract_address().into_val(&env),
                VoteContext::RetrospectiveGrant.into_val(&env),
                count.into_val(&env),
                options.into_val(&env),
                now.into_val(&env),
                (now + voting_duration).into_val(&env),
                None::<u32>.into_val(&env),
                None::<u32>.into_val(&env),
                true.into_val(&env),
            ],
        );
        let session_id: BytesN<32> =
            env.invoke_contract(&gov_addr, &sym(&env, "create_session"), gov_args);

        env.storage()
            .persistent()
            .set(&DataKey::RetroSession(count), &session_id);

        let grant = Grant {
            id: count,
            creator: creator.clone(),
            grant_type: GrantType::Retrospective,
            status: GrantStatus::Active,
            amount,
            asset,
            pool_id,
            milestone_count: 0,
            metadata_cid: String::from_str(&env, ""),
            created_at: env.ledger().timestamp(),
        };

        let grant_key = DataKey::Grant(count);
        env.storage().persistent().set(&grant_key, &grant);
        Self::extend_persistent_ttl(&env, &grant_key);
        Self::extend_instance_ttl(&env);

        GrantCreated {
            id: count,
            grant_type: GrantType::Retrospective,
            creator,
            amount,
        }
        .publish(&env);

        Ok(count)
    }

    pub fn finalize_retrospective(
        env: Env,
        grant_id: u64,
        recipients: Vec<Address>,
    ) -> Result<(), Error> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let mut grant: Grant = env
            .storage()
            .persistent()
            .get(&DataKey::Grant(grant_id))
            .ok_or(Error::GrantNotFound)?;

        if grant.grant_type != GrantType::Retrospective {
            return Err(Error::NotRetrospectiveGrant);
        }
        if grant.status != GrantStatus::Active {
            return Err(Error::GrantNotActive);
        }

        let session_id: BytesN<32> = env
            .storage()
            .persistent()
            .get(&DataKey::RetroSession(grant_id))
            .ok_or(Error::NoVoteSession)?;

        // Get voting results
        let gov_addr = Self::get_gov_addr(&env);
        let result_args: Vec<Val> = Vec::from_array(&env, [session_id.into_val(&env)]);
        let results: Vec<VoteOption> =
            env.invoke_contract(&gov_addr, &sym(&env, "get_result"), result_args);

        let mut total_votes: u64 = 0;
        for opt in results.iter() {
            total_votes += opt.weighted_votes;
        }

        let escrow_addr = Self::get_escrow_addr(&env);
        let rep_addr = Self::get_rep_addr(&env);

        if total_votes > 0 {
            for (i, opt) in results.iter().enumerate() {
                if opt.weighted_votes > 0 && (i as u32) < recipients.len() {
                    let share = grant
                        .amount
                        .checked_mul(opt.weighted_votes as i128)
                        .ok_or(Error::Overflow)?
                        / total_votes as i128;
                    if share > 0 {
                        let recipient =
                            recipients.get(i as u32).ok_or(Error::InvalidProjectIndex)?;

                        let release_args: Vec<Val> = Vec::from_array(
                            &env,
                            [
                                grant.pool_id.clone().into_val(&env),
                                recipient.clone().into_val(&env),
                                share.into_val(&env),
                            ],
                        );
                        env.invoke_contract::<()>(
                            &escrow_addr,
                            &sym(&env, "release_partial"),
                            release_args,
                        );

                        let rep_args: Vec<Val> = Vec::from_array(
                            &env,
                            [
                                env.current_contract_address().into_val(&env),
                                recipient.into_val(&env),
                                share.into_val(&env),
                            ],
                        );
                        env.invoke_contract::<()>(
                            &rep_addr,
                            &sym(&env, "record_grant_received"),
                            rep_args,
                        );
                    }
                }
            }
        }

        grant.status = GrantStatus::Completed;
        env.storage()
            .persistent()
            .set(&DataKey::Grant(grant_id), &grant);

        GrantCompleted { grant_id }.publish(&env);
        Ok(())
    }

    // ========================================================================
    // QUADRATIC FUNDING ROUND
    // ========================================================================

    pub fn create_qf_round(
        env: Env,
        creator: Address,
        matching_pool: i128,
        asset: Address,
        project_names: Vec<String>,
        duration: u64,
    ) -> Result<u64, Error> {
        creator.require_auth();

        if matching_pool <= 0 {
            return Err(Error::InvalidAmount);
        }

        let count = Self::next_grant_id(&env);
        let escrow_addr = Self::get_escrow_addr(&env);

        let pool_args: Vec<Val> = Vec::from_array(
            &env,
            [
                creator.clone().into_val(&env),
                ModuleType::Grant.into_val(&env),
                count.into_val(&env),
                matching_pool.into_val(&env),
                asset.clone().into_val(&env),
                (env.ledger().timestamp() + 31_536_000).into_val(&env),
                env.current_contract_address().into_val(&env),
            ],
        );
        let pool_id: BytesN<32> =
            env.invoke_contract(&escrow_addr, &sym(&env, "create_pool"), pool_args);

        let lock_args: Vec<Val> = Vec::from_array(&env, [pool_id.clone().into_val(&env)]);
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "lock_pool"), lock_args);

        // Create QF session
        let gov_addr = Self::get_gov_addr(&env);
        let now = env.ledger().timestamp();
        let gov_args: Vec<Val> = Vec::from_array(
            &env,
            [
                env.current_contract_address().into_val(&env),
                VoteContext::QFRound.into_val(&env),
                count.into_val(&env),
                project_names.clone().into_val(&env),
                now.into_val(&env),
                (now + duration).into_val(&env),
                None::<u32>.into_val(&env),
                None::<u32>.into_val(&env),
                false.into_val(&env),
            ],
        );
        let session_id: BytesN<32> =
            env.invoke_contract(&gov_addr, &sym(&env, "create_session"), gov_args);

        let qf_data = QFRoundData {
            session_id,
            matching_pool,
            project_count: project_names.len(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::QFRound(count), &qf_data);

        let grant = Grant {
            id: count,
            creator: creator.clone(),
            grant_type: GrantType::QF,
            status: GrantStatus::Active,
            amount: matching_pool,
            asset,
            pool_id,
            milestone_count: 0,
            metadata_cid: String::from_str(&env, ""),
            created_at: env.ledger().timestamp(),
        };

        let grant_key = DataKey::Grant(count);
        env.storage().persistent().set(&grant_key, &grant);
        Self::extend_persistent_ttl(&env, &grant_key);
        Self::extend_instance_ttl(&env);

        GrantCreated {
            id: count,
            grant_type: GrantType::QF,
            creator,
            amount: matching_pool,
        }
        .publish(&env);

        Ok(count)
    }

    pub fn donate_to_project(
        env: Env,
        grant_id: u64,
        amount: i128,
        project_index: u32,
    ) -> Result<(), Error> {
        let grant: Grant = env
            .storage()
            .persistent()
            .get(&DataKey::Grant(grant_id))
            .ok_or(Error::GrantNotFound)?;

        if grant.grant_type != GrantType::QF {
            return Err(Error::NotQFGrant);
        }

        let qf_data: QFRoundData = env
            .storage()
            .persistent()
            .get(&DataKey::QFRound(grant_id))
            .ok_or(Error::GrantNotFound)?;

        if project_index >= qf_data.project_count {
            return Err(Error::InvalidProjectIndex);
        }

        // Record donation in GovernanceVoting
        let gov_addr = Self::get_gov_addr(&env);
        let args: Vec<Val> = Vec::from_array(
            &env,
            [
                qf_data.session_id.into_val(&env),
                env.current_contract_address().into_val(&env),
                amount.into_val(&env),
                project_index.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&gov_addr, &sym(&env, "record_qf_donation"), args);

        QFDonationMade {
            grant_id,
            project_index,
            amount,
        }
        .publish(&env);

        Ok(())
    }

    pub fn finalize_qf_round(
        env: Env,
        grant_id: u64,
        project_addresses: Vec<Address>,
    ) -> Result<(), Error> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let mut grant: Grant = env
            .storage()
            .persistent()
            .get(&DataKey::Grant(grant_id))
            .ok_or(Error::GrantNotFound)?;

        if grant.grant_type != GrantType::QF {
            return Err(Error::NotQFGrant);
        }
        if grant.status != GrantStatus::Active {
            return Err(Error::GrantNotActive);
        }

        let qf_data: QFRoundData = env
            .storage()
            .persistent()
            .get(&DataKey::QFRound(grant_id))
            .ok_or(Error::GrantNotFound)?;

        let gov_addr = Self::get_gov_addr(&env);
        let dist_args: Vec<Val> = Vec::from_array(
            &env,
            [
                qf_data.session_id.into_val(&env),
                qf_data.matching_pool.into_val(&env),
            ],
        );
        let distributions: Vec<(u32, i128)> =
            env.invoke_contract(&gov_addr, &sym(&env, "compute_qf_distribution"), dist_args);

        let escrow_addr = Self::get_escrow_addr(&env);
        let rep_addr = Self::get_rep_addr(&env);

        for (index, amount) in distributions.iter() {
            if amount > 0 {
                let addr = project_addresses
                    .get(index)
                    .ok_or(Error::InvalidProjectIndex)?;

                let release_args: Vec<Val> = Vec::from_array(
                    &env,
                    [
                        grant.pool_id.clone().into_val(&env),
                        addr.clone().into_val(&env),
                        amount.into_val(&env),
                    ],
                );
                env.invoke_contract::<()>(
                    &escrow_addr,
                    &sym(&env, "release_partial"),
                    release_args,
                );

                let rep_args: Vec<Val> = Vec::from_array(
                    &env,
                    [
                        env.current_contract_address().into_val(&env),
                        addr.into_val(&env),
                        amount.into_val(&env),
                    ],
                );
                env.invoke_contract::<()>(&rep_addr, &sym(&env, "record_grant_received"), rep_args);
            }
        }

        grant.status = GrantStatus::Completed;
        env.storage()
            .persistent()
            .set(&DataKey::Grant(grant_id), &grant);

        GrantCompleted { grant_id }.publish(&env);
        Ok(())
    }

    // ========================================================================
    // CANCEL GRANT
    // ========================================================================

    pub fn cancel_grant(env: Env, creator: Address, grant_id: u64) -> Result<(), Error> {
        creator.require_auth();

        let mut grant: Grant = env
            .storage()
            .persistent()
            .get(&DataKey::Grant(grant_id))
            .ok_or(Error::GrantNotFound)?;

        if grant.creator != creator {
            return Err(Error::NotCreator);
        }

        if grant.status != GrantStatus::Pending && grant.status != GrantStatus::Active {
            return Err(Error::CannotCancel);
        }

        let escrow_addr = Self::get_escrow_addr(&env);
        let args: Vec<Val> = Vec::from_array(&env, [grant.pool_id.into_val(&env)]);
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "refund_all"), args);

        grant.status = GrantStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Grant(grant_id), &grant);

        Ok(())
    }

    // ========================================================================
    // UPGRADE
    // ========================================================================

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Self::extend_instance_ttl(&env);
        Ok(())
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

    fn next_grant_id(env: &Env) -> u64 {
        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::GrantCount)
            .unwrap_or(0);
        count += 1;
        env.storage().instance().set(&DataKey::GrantCount, &count);
        count
    }

    fn require_admin(env: &Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    fn get_escrow_addr(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .expect("not initialized")
    }

    fn get_rep_addr(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::ReputationRegistry)
            .expect("not initialized")
    }

    fn get_gov_addr(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::GovernanceVoting)
            .expect("not initialized")
    }
}
