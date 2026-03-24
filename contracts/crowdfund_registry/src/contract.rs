use crate::error::Error;
use crate::events::{CampaignCreated, CampaignFunded, MilestoneFinalized, PledgeRecorded};
use crate::storage::{Campaign, CampaignStatus, DataKey, Milestone, MilestoneStatus};
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, IntoVal, String, Symbol, Vec};

use core_escrow::ModuleType;

#[contract]
pub struct CrowdfundRegistry;

#[contractimpl]
impl CrowdfundRegistry {
    pub fn init_crowdfund_reg(
        env: Env,
        admin: Address,
        project_registry: Address,
        core_escrow: Address,
        voting_contract: Address,
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
            .set(&DataKey::VotingContract, &voting_contract);
        env.storage()
            .instance()
            .set(&DataKey::ReputationRegistry, &reputation_registry);
        env.storage()
            .instance()
            .set(&DataKey::PaymentRouter, &payment_router);
        env.storage().instance().set(&DataKey::CampaignCount, &0u64);
        Ok(())
    }

    pub fn create_campaign(
        env: Env,
        owner: Address,
        project_id: u64,
        metadata_cid: String,
        funding_goal: i128,
        asset: Address,
        deadline: u64,
        milestones: Vec<Milestone>,
        min_pledge: i128,
    ) -> Result<u64, Error> {
        owner.require_auth();

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::CampaignCount)
            .unwrap_or(0);
        count += 1;
        env.storage()
            .instance()
            .set(&DataKey::CampaignCount, &count);

        let esc_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;

        let pool_id: BytesN<32> = env.invoke_contract(
            &esc_addr,
            &Symbol::new(&env, "create_pool"),
            (
                owner.clone(),
                ModuleType::Crowdfund,
                count,
                0i128, // Initial deposit is 0
                asset.clone(),
                deadline,
                env.current_contract_address(),
            )
                .into_val(&env),
        );

        let campaign = Campaign {
            id: count,
            owner: owner.clone(),
            project_id,
            metadata_cid,
            status: CampaignStatus::Campaigning,
            funding_goal,
            current_funding: 0,
            asset,
            pool_id,
            deadline,
            milestones,
            min_pledge,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Campaign(count), &campaign);
        CampaignCreated {
            id: count,
            owner,
            funding_goal,
        }
        .publish(&env);
        Ok(count)
    }

    pub fn pledge(env: Env, donor: Address, campaign_id: u64, amount: i128) -> Result<(), Error> {
        donor.require_auth();

        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&DataKey::Campaign(campaign_id))
            .ok_or(Error::CampaignNotFound)?;

        if env.ledger().timestamp() > campaign.deadline {
            return Err(Error::DeadlinePassed);
        }
        if campaign.status != CampaignStatus::Campaigning {
            return Err(Error::NotCampaigning);
        }
        if amount < campaign.min_pledge {
            return Err(Error::BelowMinPledge);
        }

        let router_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::PaymentRouter)
            .ok_or(Error::NotInitialized)?;

        let net_amount: i128 = env.invoke_contract(
            &router_addr,
            &Symbol::new(&env, "route_deposit"),
            (
                donor.clone(),
                amount,
                campaign.asset.clone(),
                ModuleType::Crowdfund,
            )
                .into_val(&env),
        );

        let esc_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;
        env.invoke_contract::<()>(
            &esc_addr,
            &Symbol::new(&env, "deposit"),
            (
                campaign.pool_id.clone(),
                net_amount,
                campaign.asset.clone(),
                donor.clone(),
            )
                .into_val(&env),
        );

        campaign.current_funding += net_amount;

        let mut pledge_amount: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Pledge(campaign_id, donor.clone()))
            .unwrap_or(0);
        pledge_amount += net_amount;
        env.storage()
            .persistent()
            .set(&DataKey::Pledge(campaign_id, donor.clone()), &pledge_amount);

        let mut donors: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Donors(campaign_id))
            .unwrap_or(Vec::new(&env));
        if !donors.contains(&donor) {
            donors.push_back(donor.clone());
            env.storage()
                .persistent()
                .set(&DataKey::Donors(campaign_id), &donors);
        }

        if campaign.current_funding >= campaign.funding_goal {
            campaign.status = CampaignStatus::Funded;

            let esc_addr: Address = env
                .storage()
                .instance()
                .get(&DataKey::CoreEscrow)
                .ok_or(Error::NotInitialized)?;
            env.invoke_contract::<()>(
                &esc_addr,
                &Symbol::new(&env, "lock_pool"),
                (campaign.pool_id.clone(),).into_val(&env),
            );

            CampaignFunded { id: campaign_id }.publish(&env);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Campaign(campaign_id), &campaign);
        PledgeRecorded {
            campaign_id,
            donor,
            amount: net_amount,
        }
        .publish(&env);
        Ok(())
    }

    pub fn submit_milestone(env: Env, campaign_id: u64, milestone_id: u32) -> Result<(), Error> {
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&DataKey::Campaign(campaign_id))
            .ok_or(Error::CampaignNotFound)?;
        campaign.owner.require_auth();

        if campaign.status != CampaignStatus::Funded && campaign.status != CampaignStatus::Executing
        {
            return Err(Error::InvalidState);
        }

        let mut found = false;
        let mut updated_milestones = Vec::new(&env);
        for m in campaign.milestones.iter() {
            let mut m_clone = m.clone();
            if m.id == milestone_id {
                if m.status != MilestoneStatus::Pending && m.status != MilestoneStatus::Rejected {
                    return Err(Error::MilestoneNotPending);
                }
                m_clone.status = MilestoneStatus::Submitted;
                found = true;
            }
            updated_milestones.push_back(m_clone);
        }

        if !found {
            return Err(Error::MilestoneNotFound);
        }

        campaign.milestones = updated_milestones;
        campaign.status = CampaignStatus::Executing;
        env.storage()
            .persistent()
            .set(&DataKey::Campaign(campaign_id), &campaign);
        Ok(())
    }

    pub fn approve_milestone(env: Env, campaign_id: u64, milestone_id: u32) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&DataKey::Campaign(campaign_id))
            .ok_or(Error::CampaignNotFound)?;

        let mut amount_to_release: i128 = 0;
        let mut updated_milestones = Vec::new(&env);
        for m in campaign.milestones.iter() {
            let mut m_clone = m.clone();
            if m.id == milestone_id {
                if m.status != MilestoneStatus::Submitted {
                    return Err(Error::MilestoneNotSubmitted);
                }
                m_clone.status = MilestoneStatus::Approved;
                amount_to_release = m.amount;
            }
            updated_milestones.push_back(m_clone);
        }

        if amount_to_release == 0 {
            return Err(Error::MilestoneAmountZero);
        }

        campaign.milestones = updated_milestones;

        // Release from CoreEscrow
        let esc_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;
        env.invoke_contract::<()>(
            &esc_addr,
            &Symbol::new(&env, "release_partial"),
            (
                campaign.pool_id.clone(),
                campaign.owner.clone(),
                amount_to_release,
            )
                .into_val(&env),
        );

        // Mark as released
        let mut finalized_milestones = Vec::new(&env);
        for m in campaign.milestones.iter() {
            let mut m_clone = m.clone();
            if m.id == milestone_id {
                m_clone.status = MilestoneStatus::Released;
            }
            finalized_milestones.push_back(m_clone);
        }
        campaign.milestones = finalized_milestones;

        // Check if all finished
        let mut all_done = true;
        for m in campaign.milestones.iter() {
            if m.status != MilestoneStatus::Released {
                all_done = false;
                break;
            }
        }
        if all_done {
            campaign.status = CampaignStatus::Completed;
        }

        env.storage()
            .persistent()
            .set(&DataKey::Campaign(campaign_id), &campaign);
        MilestoneFinalized {
            campaign_id,
            milestone_id,
        }
        .publish(&env);
        Ok(())
    }

    pub fn request_refund(env: Env, donor: Address, campaign_id: u64) -> Result<(), Error> {
        donor.require_auth();

        let campaign: Campaign = env
            .storage()
            .persistent()
            .get(&DataKey::Campaign(campaign_id))
            .ok_or(Error::CampaignNotFound)?;

        if campaign.status == CampaignStatus::Funded
            || campaign.status == CampaignStatus::Executing
            || campaign.status == CampaignStatus::Completed
        {
            return Err(Error::CampaignAlreadyFunded);
        }

        // If deadline not passed, only allowed if cancelled
        if env.ledger().timestamp() <= campaign.deadline
            && campaign.status != CampaignStatus::Cancelled
        {
            return Err(Error::CampaignActive);
        }

        let amount: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Pledge(campaign_id, donor.clone()))
            .ok_or(Error::NoPledgeFound)?;
        if amount == 0 {
            return Err(Error::AlreadyRefunded);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Pledge(campaign_id, donor.clone()), &0i128);

        // Refund from CoreEscrow
        let esc_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;

        env.invoke_contract::<()>(
            &esc_addr,
            &Symbol::new(&env, "release_partial"),
            (campaign.pool_id.clone(), donor.clone(), amount).into_val(&env),
        );
        Ok(())
    }

    pub fn get_campaign(env: Env, id: u64) -> Result<Campaign, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Campaign(id))
            .ok_or(Error::CampaignNotFound)
    }

    // ========================================
    // QUERY FUNCTIONS
    // ========================================

    pub fn get_admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_project_reg(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::ProjectRegistry)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_core_escrow(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_voting_contract(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::VotingContract)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_reputation_reg(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::ReputationRegistry)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_payment_router(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::PaymentRouter)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_fee_account(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::FeeAccount)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_treasury(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Treasury)
            .ok_or(Error::NotInitialized)
    }

    // ========================================
    // ADMINISTRATIVE FUNCTIONS
    // ========================================

    pub fn update_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        if Self::is_zero_address(&env, &new_admin) {
            panic!("new admin cannot be zero address");
        }

        env.storage().instance().set(&DataKey::Admin, &new_admin);
        Ok(())
    }

    pub fn update_fee_account(env: Env, new_fee_account: Address) -> Result<(), Error> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        if Self::is_zero_address(&env, &new_fee_account) {
            panic!("new fee account cannot be zero address");
        }

        env.storage()
            .instance()
            .set(&DataKey::FeeAccount, &new_fee_account);
        Ok(())
    }

    pub fn update_treasury(env: Env, new_treasury: Address) -> Result<(), Error> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        if Self::is_zero_address(&env, &new_treasury) {
            panic!("new treasury cannot be zero address");
        }

        env.storage()
            .instance()
            .set(&DataKey::Treasury, &new_treasury);
        Ok(())
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    // ========================================
    // INTERNAL HELPERS
    // ========================================

    fn is_zero_address(_env: &Env, _address: &Address) -> bool {
        // Placeholder as Soroban lacks a native zero address.
        false
    }
}
