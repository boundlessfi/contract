use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

use boundless_types::ModuleType;
use core_escrow::CoreEscrowClient;
use governance_voting::storage::VoteContext;
use governance_voting::GovernanceVotingClient;
use reputation_registry::ReputationRegistryClient;

use crate::error::Error;
use crate::events::{GrantCompleted, GrantCreated, MilestoneApproved, MilestoneSubmitted, QFDonationMade};
use crate::storage::{
    DataKey, Grant, GrantMilestone, GrantStatus, GrantType, MilestoneStatus, QFRoundData,
};

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
        Ok(())
    }

    // ========================================================================
    // QUERIES
    // ========================================================================

    pub fn get_grant(env: Env, grant_id: u64) -> Result<Grant, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Grant(grant_id))
            .ok_or(Error::GrantNotFound)
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
        milestone_descs: Vec<(String, u32)>, // (description, pct in basis points)
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
        let escrow = Self::escrow_client(&env);

        let pool_id = escrow.create_pool(
            &creator,
            &ModuleType::Grant,
            &count,
            &amount,
            &asset,
            &(env.ledger().timestamp() + 31_536_000), // 1 year
            &env.current_contract_address(),
        );

        // Define release slots based on milestone percentages
        let mut slots: Vec<(Address, i128)> = Vec::new(&env);
        let milestone_count = milestone_descs.len();
        for (_, pct) in milestone_descs.iter() {
            let slot_amount = (amount * pct as i128) / 10000;
            slots.push_back((recipient.clone(), slot_amount));
        }

        escrow.define_release_slots(&pool_id, &slots);
        escrow.lock_pool(&pool_id);

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

        env.storage()
            .persistent()
            .set(&DataKey::Grant(count), &grant);

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
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
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
        let escrow = Self::escrow_client(&env);
        escrow.release_slot(&grant.pool_id, &milestone_index);

        MilestoneApproved {
            grant_id,
            milestone_index,
        }
        .publish(&env);

        // Check if all milestones are released
        let mut all_done = true;
        for i in 0..grant.milestone_count {
            if i == milestone_index {
                continue; // already updated above
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

            // Record reputation for grant recipient
            let rep = Self::rep_client(&env);
            rep.record_grant_received(
                &env.current_contract_address(),
                &Self::get_slot_recipient(&env, &grant.pool_id),
                &grant.amount,
            );

            GrantCompleted { grant_id }.publish(&env);
        } else {
            // Update grant status to Executing if not already
            if grant.status == GrantStatus::Active {
                grant.status = GrantStatus::Executing;
                env.storage()
                    .persistent()
                    .set(&DataKey::Grant(grant_id), &grant);
            }
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
        let escrow = Self::escrow_client(&env);

        let pool_id = escrow.create_pool(
            &creator,
            &ModuleType::Grant,
            &count,
            &amount,
            &asset,
            &(env.ledger().timestamp() + 31_536_000),
            &env.current_contract_address(),
        );

        escrow.lock_pool(&pool_id);

        // Create governance voting session
        let gov = Self::gov_client(&env);
        let now = env.ledger().timestamp();
        let session_id = gov.create_session(
            &env.current_contract_address(),
            &VoteContext::RetrospectiveGrant,
            &count,
            &options,
            &now,
            &(now + voting_duration),
            &None::<u32>,
            &None::<u32>,
            &true, // weight_by_reputation
        );

        // Store session_id for this grant
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

        env.storage()
            .persistent()
            .set(&DataKey::Grant(count), &grant);

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
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
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

        // Get voting results to distribute proportionally
        let gov = Self::gov_client(&env);
        let results = gov.get_result(&session_id);

        // Calculate total weighted votes
        let mut total_votes: u64 = 0;
        for opt in results.iter() {
            total_votes += opt.weighted_votes;
        }

        let escrow = Self::escrow_client(&env);
        let rep = Self::rep_client(&env);

        if total_votes > 0 {
            for (i, opt) in results.iter().enumerate() {
                if opt.weighted_votes > 0 && (i as u32) < recipients.len() {
                    let share =
                        (grant.amount * opt.weighted_votes as i128) / total_votes as i128;
                    if share > 0 {
                        let recipient = recipients.get(i as u32).ok_or(Error::InvalidProjectIndex)?;
                        escrow.release_partial(
                            &grant.pool_id,
                            &recipient,
                            &share,
                        );
                        rep.record_grant_received(
                            &env.current_contract_address(),
                            &recipient,
                            &share,
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
        let escrow = Self::escrow_client(&env);

        let pool_id = escrow.create_pool(
            &creator,
            &ModuleType::Grant,
            &count,
            &matching_pool,
            &asset,
            &(env.ledger().timestamp() + 31_536_000),
            &env.current_contract_address(),
        );

        escrow.lock_pool(&pool_id);

        // Create governance voting QF session
        let gov = Self::gov_client(&env);
        let now = env.ledger().timestamp();
        let session_id = gov.create_session(
            &env.current_contract_address(),
            &VoteContext::QFRound,
            &count,
            &project_names,
            &now,
            &(now + duration),
            &None::<u32>,
            &None::<u32>,
            &false,
        );

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

        env.storage()
            .persistent()
            .set(&DataKey::Grant(count), &grant);

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
        let gov = Self::gov_client(&env);
        gov.record_qf_donation(
            &qf_data.session_id,
            &env.current_contract_address(),
            &amount,
            &project_index,
        );

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
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
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

        let gov = Self::gov_client(&env);
        let distributions = gov.compute_qf_distribution(
            &qf_data.session_id,
            &qf_data.matching_pool,
        );

        let escrow = Self::escrow_client(&env);
        let rep = Self::rep_client(&env);

        for (index, amount) in distributions.iter() {
            if amount > 0 {
                let addr = project_addresses.get(index).ok_or(Error::InvalidProjectIndex)?;
                escrow.release_partial(&grant.pool_id, &addr, &amount);
                rep.record_grant_received(
                    &env.current_contract_address(),
                    &addr,
                    &amount,
                );
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
    // INTERNAL HELPERS
    // ========================================================================

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

    fn escrow_client(env: &Env) -> CoreEscrowClient<'_> {
        let addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .expect("not initialized");
        CoreEscrowClient::new(env, &addr)
    }

    fn rep_client(env: &Env) -> ReputationRegistryClient<'_> {
        let addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::ReputationRegistry)
            .expect("not initialized");
        ReputationRegistryClient::new(env, &addr)
    }

    fn gov_client(env: &Env) -> GovernanceVotingClient<'_> {
        let addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::GovernanceVoting)
            .expect("not initialized");
        GovernanceVotingClient::new(env, &addr)
    }

    fn get_slot_recipient(env: &Env, pool_id: &BytesN<32>) -> Address {
        // Get the recipient from slot 0 (all milestone slots have the same recipient)
        let escrow = Self::escrow_client(env);
        let slot = escrow.get_slot(pool_id, &0u32);
        slot.recipient
    }
}
