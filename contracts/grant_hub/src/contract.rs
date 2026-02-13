use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, IntoVal, String, Symbol, Vec};

use core_escrow::ModuleType;
use governance_voting::storage::VoteContext;
use reputation_registry::ActivityCategory;

use crate::error::Error;
use crate::events::GrantCreated;
use crate::storage::{DataKey, Grant, GrantMilestone, GrantStatus, GrantType, MilestoneStatus};

#[contract]
pub struct GrantHub;

#[contractimpl]
impl GrantHub {
    pub fn init_grant_hub(
        env: Env,
        admin: Address,
        project_registry: Address,
        core_escrow: Address,
        governance_voting: Address,
        reputation_registry: Address,
        payment_router: Address,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::ProjectRegistry, &project_registry);
        env.storage()
            .instance()
            .set(&DataKey::CoreEscrow, &core_escrow);
        env.storage()
            .instance()
            .set(&DataKey::GovernanceVoting, &governance_voting);
        env.storage()
            .instance()
            .set(&DataKey::ReputationRegistry, &reputation_registry);
        env.storage()
            .instance()
            .set(&DataKey::PaymentRouter, &payment_router);
        env.storage().instance().set(&DataKey::GrantCount, &0u64);
        Ok(())
    }

    pub fn create_milestone_grant(
        env: Env,
        creator: Address,
        project_id: u64,
        recipient: Address,
        metadata_cid: String,
        total_budget: i128,
        asset: Address,
        milestone_inputs: Vec<(String, i128)>, // (description, amount)
    ) -> Result<u64, Error> {
        creator.require_auth();

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::GrantCount)
            .unwrap_or(0);
        count += 1;
        env.storage().instance().set(&DataKey::GrantCount, &count);

        let esc_addr: Address = env.storage().instance().get(&DataKey::CoreEscrow).ok_or(Error::NotInitialized)?;

        let pool_id: BytesN<32> = env.invoke_contract(
            &esc_addr,
            &Symbol::new(&env, "create_pool"),
            (
                creator.clone(),
                ModuleType::Grant,
                count,
                total_budget,
                asset.clone(),
                env.ledger().timestamp() + 31536000,
                env.current_contract_address(),
            )
                .into_val(&env),
        );

        let mut milestones = Vec::new(&env);
        let mut slots = Vec::new(&env);
        for (i, (desc, amt)) in milestone_inputs.iter().enumerate() {
            milestones.push_back(GrantMilestone {
                index: i as u32,
                description_cid: desc,
                amount: amt,
                status: MilestoneStatus::Pending,
                submission_cid: None,
            });
            slots.push_back((recipient.clone(), amt));
        }

        env.invoke_contract::<()>(
            &esc_addr,
            &Symbol::new(&env, "define_release_slots"),
            (pool_id.clone(), slots).into_val(&env),
        );

        env.invoke_contract::<()>(
            &esc_addr,
            &Symbol::new(&env, "lock_pool"),
            (pool_id.clone(),).into_val(&env),
        );

        let grant = Grant {
            id: count,
            grant_type: GrantType::Milestone,
            creator: creator.clone(),
            project_id,
            metadata_cid: metadata_cid.clone(),
            status: GrantStatus::Active,
            total_budget,
            asset,
            pool_id,
            recipient: Some(recipient),
            milestones,
            vote_session_id: None,
            applicants: Vec::new(&env),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Grant(count), &grant);

        GrantCreated {
            id: count,
            grant_type: GrantType::Milestone,
            creator,
            budget: total_budget,
        }
        .publish(&env);

        Ok(count)
    }

    pub fn submit_grant_milestone(
        env: Env,
        recipient: Address,
        grant_id: u64,
        milestone_index: u32,
        submission_cid: String,
    ) -> Result<(), Error> {
        recipient.require_auth();

        let mut grant: Grant = env
            .storage()
            .persistent()
            .get(&DataKey::Grant(grant_id))
            .ok_or(Error::GrantNotFound)?;

        if grant.grant_type != GrantType::Milestone {
            return Err(Error::NotMilestoneGrant);
        }
        if grant.recipient.as_ref().ok_or(Error::NotRecipient)? != &recipient {
            return Err(Error::NotRecipient);
        }

        let mut found = false;
        let mut updated_milestones = Vec::new(&env);
        for m in grant.milestones.iter() {
            let mut m = m;
            if m.index == milestone_index {
                if m.status != MilestoneStatus::Pending && m.status != MilestoneStatus::Rejected {
                    return Err(Error::InvalidMilestoneStatus);
                }
                m.status = MilestoneStatus::Submitted;
                m.submission_cid = Some(submission_cid.clone());
                found = true;
            }
            updated_milestones.push_back(m);
        }

        if !found {
            return Err(Error::MilestoneNotFound);
        }

        grant.milestones = updated_milestones;
        env.storage()
            .persistent()
            .set(&DataKey::Grant(grant_id), &grant);
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

        let mut amount_to_release = 0;
        let mut updated_milestones = Vec::new(&env);
        let mut completed_count = 0;
        let mut found = false;

        for m in grant.milestones.iter() {
            let mut m = m;
            if m.index == milestone_index {
                if m.status != MilestoneStatus::Submitted {
                    return Err(Error::MilestoneNotSubmitted);
                }
                m.status = MilestoneStatus::Approved;
                amount_to_release = m.amount;
                found = true;
            }
            if m.status == MilestoneStatus::Approved {
                completed_count += 1;
            }
            updated_milestones.push_back(m);
        }

        if !found {
            return Err(Error::MilestoneNotFound);
        }

        grant.milestones = updated_milestones;
        if completed_count == grant.milestones.len() {
            grant.status = GrantStatus::Completed;
        }

        env.storage()
            .persistent()
            .set(&DataKey::Grant(grant_id), &grant);

        let esc_addr: Address = env.storage().instance().get(&DataKey::CoreEscrow).ok_or(Error::NotInitialized)?;
        env.invoke_contract::<()>(
            &esc_addr,
            &Symbol::new(&env, "release_slot"),
            (grant.pool_id.clone(), milestone_index).into_val(&env),
        );

        let rep_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::ReputationRegistry)
            .ok_or(Error::NotInitialized)?;
        let points = (amount_to_release / 100) as u32; // Scale points proportionally
        env.invoke_contract::<()>(
            &rep_addr,
            &Symbol::new(&env, "record_completion"),
            (
                env.current_contract_address(),
                grant.recipient.ok_or(Error::NotRecipient)?,
                grant_id,
                ActivityCategory::Development,
                points.max(1), // Ensure at least 1 point
                false,         // is_hackathon
                false,         // is_win
            )
                .into_val(&env),
        );
        Ok(())
    }

    pub fn create_retrospective_grant(
        env: Env,
        creator: Address,
        project_id: u64,
        metadata_cid: String,
        total_budget: i128,
        asset: Address,
        applicants: Vec<Address>,
    ) -> Result<u64, Error> {
        creator.require_auth();

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::GrantCount)
            .unwrap_or(0);
        count += 1;
        env.storage().instance().set(&DataKey::GrantCount, &count);

        let esc_addr: Address = env.storage().instance().get(&DataKey::CoreEscrow).ok_or(Error::NotInitialized)?;
        let pool_id: BytesN<32> = env.invoke_contract(
            &esc_addr,
            &Symbol::new(&env, "create_pool"),
            (
                creator.clone(),
                ModuleType::Grant,
                count,
                total_budget,
                asset.clone(),
                env.ledger().timestamp() + 31536000,
                env.current_contract_address(),
            )
                .into_val(&env),
        );

        let gov_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::GovernanceVoting)
            .ok_or(Error::NotInitialized)?;
        let mut options = Vec::new(&env);
        for _ in 0..applicants.len() {
            options.push_back(String::from_str(&env, "Applicant"));
        }

        let session_id: BytesN<32> = env.invoke_contract(
            &gov_addr,
            &Symbol::new(&env, "create_session"),
            (
                env.current_contract_address(),
                VoteContext::RetrospectiveGrant,
                count,
                options,
                env.ledger().timestamp(),
                env.ledger().timestamp() + 604800,
                None::<u32>,
                true, // weight_by_reputation
            )
                .into_val(&env),
        );

        let grant = Grant {
            id: count,
            grant_type: GrantType::Retrospective,
            creator,
            project_id,
            metadata_cid,
            status: GrantStatus::Voting,
            total_budget,
            asset,
            pool_id,
            recipient: None,
            milestones: Vec::new(&env),
            vote_session_id: Some(session_id),
            applicants,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Grant(count), &grant);
        Ok(count)
    }

    pub fn finalize_retrospective(env: Env, grant_id: u64) -> Result<(), Error> {
        let mut grant: Grant = env
            .storage()
            .persistent()
            .get(&DataKey::Grant(grant_id))
            .ok_or(Error::GrantNotFound)?;

        if grant.grant_type != GrantType::Retrospective {
            return Err(Error::NotRetrospectiveGrant);
        }

        let gov_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::GovernanceVoting)
            .ok_or(Error::NotInitialized)?;
        let winner_index: u32 = env.invoke_contract(
            &gov_addr,
            &Symbol::new(&env, "get_winning_option"),
            (grant.vote_session_id.clone().ok_or(Error::GrantNotFound)?,).into_val(&env),
        );

        let winner = grant.applicants.get(winner_index).unwrap();
        grant.recipient = Some(winner.clone());
        grant.status = GrantStatus::Completed;

        env.storage()
            .persistent()
            .set(&DataKey::Grant(grant_id), &grant);

        let esc_addr: Address = env.storage().instance().get(&DataKey::CoreEscrow).ok_or(Error::NotInitialized)?;
        env.invoke_contract::<()>(
            &esc_addr,
            &Symbol::new(&env, "release_partial"),
            (grant.pool_id, winner.clone(), grant.total_budget).into_val(&env),
        );

        // Record reputation
        let rep_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::ReputationRegistry)
            .ok_or(Error::NotInitialized)?;
        env.invoke_contract::<()>(
            &rep_addr,
            &Symbol::new(&env, "record_completion"),
            (
                env.current_contract_address(),
                winner,
                grant_id,
                ActivityCategory::Development,
                100u32, // Retrospective grants get base points
                false,
                false,
            )
                .into_val(&env),
        );
        Ok(())
    }

    pub fn create_qf_round(
        env: Env,
        creator: Address,
        project_id: u64,
        metadata_cid: String,
        matching_pool: i128,
        asset: Address,
        eligible_projects: Vec<u64>,
    ) -> Result<u64, Error> {
        creator.require_auth();

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::GrantCount)
            .unwrap_or(0);
        count += 1;
        env.storage().instance().set(&DataKey::GrantCount, &count);

        let esc_addr: Address = env.storage().instance().get(&DataKey::CoreEscrow).ok_or(Error::NotInitialized)?;
        let pool_id: BytesN<32> = env.invoke_contract(
            &esc_addr,
            &Symbol::new(&env, "create_pool"),
            (
                creator.clone(),
                ModuleType::Grant,
                count,
                matching_pool,
                asset.clone(),
                env.ledger().timestamp() + 31536000,
                env.current_contract_address(),
            )
                .into_val(&env),
        );

        let gov_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::GovernanceVoting)
            .ok_or(Error::NotInitialized)?;
        let mut options = Vec::new(&env);
        for _ in 0..eligible_projects.len() {
            options.push_back(String::from_str(&env, "Project"));
        }

        let session_id: BytesN<32> = env.invoke_contract(
            &gov_addr,
            &Symbol::new(&env, "create_session"),
            (
                env.current_contract_address(),
                VoteContext::QFRound,
                count,
                options,
                env.ledger().timestamp(),
                env.ledger().timestamp() + 2592000,
                None::<u32>,
                false, // QF weight by rep handled differently
            )
                .into_val(&env),
        );

        let grant = Grant {
            id: count,
            grant_type: GrantType::Quadratic,
            creator: creator.clone(),
            project_id,
            metadata_cid,
            status: GrantStatus::Active,
            total_budget: matching_pool,
            asset,
            pool_id,
            recipient: None,
            milestones: Vec::new(&env),
            vote_session_id: Some(session_id),
            applicants: Vec::new(&env),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Grant(count), &grant);
        Ok(count)
    }

    pub fn finalize_qf_round(
        env: Env,
        grant_id: u64,
        project_addresses: Vec<Address>,
    ) -> Result<(), Error> {
        let mut grant: Grant = env
            .storage()
            .persistent()
            .get(&DataKey::Grant(grant_id))
            .ok_or(Error::GrantNotFound)?;

        if grant.grant_type != GrantType::Quadratic {
            return Err(Error::NotQFGrant);
        }

        let gov_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::GovernanceVoting)
            .ok_or(Error::NotInitialized)?;

        let distributions: Vec<(u32, i128)> = env.invoke_contract(
            &gov_addr,
            &Symbol::new(&env, "compute_qf_distribution"),
            (grant.vote_session_id.clone().ok_or(Error::GrantNotFound)?, grant.total_budget).into_val(&env),
        );

        let esc_addr: Address = env.storage().instance().get(&DataKey::CoreEscrow).ok_or(Error::NotInitialized)?;
        for dist in distributions.iter() {
            let (index, amount) = dist;
            let addr = project_addresses.get(index).unwrap();
            if amount > 0 {
                env.invoke_contract::<()>(
                    &esc_addr,
                    &Symbol::new(&env, "release_partial"),
                    (grant.pool_id.clone(), addr.clone(), amount).into_val(&env),
                );

                // Record reputation for each funded project
                let rep_addr: Address = env
                    .storage()
                    .instance()
                    .get(&DataKey::ReputationRegistry)
                    .ok_or(Error::NotInitialized)?;
                env.invoke_contract::<()>(
                    &rep_addr,
                    &Symbol::new(&env, "record_completion"),
                    (
                        env.current_contract_address(),
                        addr,
                        grant_id,
                        ActivityCategory::Development,
                        (amount / 100) as u32,
                        false,
                        false,
                    )
                        .into_val(&env),
                );
            }
        }

        grant.status = GrantStatus::Completed;
        env.storage()
            .persistent()
            .set(&DataKey::Grant(grant_id), &grant);
        Ok(())
    }

    pub fn get_grant(env: Env, grant_id: u64) -> Result<Grant, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Grant(grant_id))
            .ok_or(Error::GrantNotFound)
    }
}
