use crate::error::Error;
use crate::events::{
    ApplicationRejected, BountyApplied, BountyAssigned, BountyCancelled, BountyCreated,
    SubmissionAccepted, WorkSubmitted,
};
use crate::storage::{Application, ApplicationStatus, Bounty, BountyStatus, BountyType, DataKey};
use soroban_sdk::{contract, contractimpl, Address, Env, String, Vec};

// External clients
use core_escrow::{CoreEscrowClient, ModuleType};
use reputation_registry::{ActivityCategory, ReputationRegistryClient};

#[contract]
pub struct BountyRegistry;

#[contractimpl]
impl BountyRegistry {
    pub fn init_bounty_reg(
        env: Env,
        admin: Address,
        core_escrow: Address,
        reputation_registry: Address,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::CoreEscrow, &core_escrow);
        env.storage()
            .instance()
            .set(&DataKey::ReputationRegistry, &reputation_registry);
        env.storage().instance().set(&DataKey::BountyCount, &0u64);
        Ok(())
    }

    pub fn create_bounty(
        env: Env,
        creator: Address,
        title: String,
        metadata_cid: String,
        model: BountyType,
        amount: i128,
        asset: Address,
        category: ActivityCategory,
        deadline: u64,
    ) -> Result<u64, Error> {
        creator.require_auth();

        if amount <= 0 {
            return Err(Error::AmountNotPositive);
        }
        if deadline <= env.ledger().timestamp() {
            return Err(Error::BountyDeadlinePassed);
        }

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::BountyCount)
            .unwrap_or(0);
        count += 1;
        env.storage().instance().set(&DataKey::BountyCount, &count);

        // Core Escrow integration
        let core_escrow_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;
        let escrow_client = CoreEscrowClient::new(&env, &core_escrow_addr);

        let pool_id = escrow_client.create_pool(
            &creator,
            &ModuleType::Bounty,
            &count,
            &amount,
            &asset,
            &deadline,
            &env.current_contract_address(),
        );

        let bounty = Bounty {
            id: count,
            creator: creator.clone(),
            title,
            metadata_cid,
            model,
            status: BountyStatus::Open,
            amount,
            asset,
            category,
            created_at: env.ledger().timestamp(),
            deadline,
            assignee: None,
            escrow_pool_id: Some(pool_id),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Bounty(count), &bounty);

        BountyCreated {
            bounty_id: count,
            creator,
        }
        .publish(&env);

        Ok(count)
    }

    pub fn apply(
        env: Env,
        applicant: Address,
        bounty_id: u64,
        proposal: String,
    ) -> Result<(), Error> {
        applicant.require_auth();

        let key = DataKey::Bounty(bounty_id);
        let bounty: Bounty = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::BountyNotFound)?;

        if bounty.status != BountyStatus::Open {
            return Err(Error::BountyNotOpen);
        }
        if env.ledger().timestamp() > bounty.deadline {
            return Err(Error::BountyDeadlinePassed);
        }

        let app_key = DataKey::Application(bounty_id, applicant.clone());
        if env.storage().persistent().has(&app_key) {
            return Err(Error::AlreadyApplied);
        }

        let app = Application {
            bounty_id,
            applicant: applicant.clone(),
            proposal,
            submitted_at: env.ledger().timestamp(),
            status: ApplicationStatus::Pending,
        };
        env.storage().persistent().set(&app_key, &app);

        BountyApplied {
            bounty_id,
            applicant,
        }
        .publish(&env);
        Ok(())
    }

    pub fn assign_bounty(
        env: Env,
        creator: Address,
        bounty_id: u64,
        applicant: Address,
    ) -> Result<(), Error> {
        creator.require_auth();

        let key = DataKey::Bounty(bounty_id);
        let mut bounty: Bounty = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::BountyNotFound)?;

        if bounty.creator != creator {
            return Err(Error::NotCreator);
        }
        if bounty.status != BountyStatus::Open {
            return Err(Error::BountyNotOpen);
        }
        if bounty.model != BountyType::Permissioned {
            return Err(Error::ActionOnlyForPermissioned);
        }

        let app_key = DataKey::Application(bounty_id, applicant.clone());
        let mut app: Application = env
            .storage()
            .persistent()
            .get(&app_key)
            .ok_or(Error::ApplicationNotFound)?;

        app.status = ApplicationStatus::Accepted;
        env.storage().persistent().set(&app_key, &app);

        bounty.assignee = Some(applicant.clone());
        bounty.status = BountyStatus::InProgress;
        env.storage().persistent().set(&key, &bounty);

        BountyAssigned {
            bounty_id,
            assignee: applicant,
        }
        .publish(&env);
        Ok(())
    }

    pub fn submit_work(
        env: Env,
        contributor: Address,
        bounty_id: u64,
        _work_cid: String,
    ) -> Result<(), Error> {
        contributor.require_auth();

        let key = DataKey::Bounty(bounty_id);
        let mut bounty: Bounty = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::BountyNotFound)?;

        if bounty.model == BountyType::Permissioned {
            if bounty.assignee != Some(contributor.clone()) {
                return Err(Error::NotAssignee);
            }
            if bounty.status != BountyStatus::InProgress {
                return Err(Error::NotInProgress);
            }
        } else {
            let app_key = DataKey::Application(bounty_id, contributor.clone());
            if !env.storage().persistent().has(&app_key) {
                return Err(Error::MustApplyBeforeSubmitting);
            }
            if bounty.status != BountyStatus::Open && bounty.status != BountyStatus::InProgress {
                return Err(Error::BountyNotOpen);
            }
        }

        if env.ledger().timestamp() > bounty.deadline {
            return Err(Error::BountyDeadlinePassed);
        }

        bounty.status = BountyStatus::InReview;
        env.storage().persistent().set(&key, &bounty);

        WorkSubmitted {
            bounty_id,
            contributor,
        }
        .publish(&env);
        Ok(())
    }

    pub fn accept_submission(
        env: Env,
        creator: Address,
        bounty_id: u64,
        winner: Address,
        rating: u32,
    ) -> Result<(), Error> {
        creator.require_auth();

        if rating > 100 {
            return Err(Error::InvalidRating);
        }

        let key = DataKey::Bounty(bounty_id);
        let mut bounty: Bounty = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::BountyNotFound)?;

        if bounty.creator != creator {
            return Err(Error::NotCreator);
        }
        if bounty.status != BountyStatus::InReview {
            return Err(Error::NotReviewable);
        }

        let app_key = DataKey::Application(bounty_id, winner.clone());
        if !env.storage().persistent().has(&app_key) {
            return Err(Error::ApplicationNotFound);
        }

        if bounty.model == BountyType::Permissioned {
            if bounty.assignee != Some(winner.clone()) {
                return Err(Error::NotAssignee);
            }
        }

        let pool_id = bounty.escrow_pool_id.clone().ok_or(Error::NoEscrowPool)?;

        // Escrow Release
        let core_escrow_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;
        let escrow_client = CoreEscrowClient::new(&env, &core_escrow_addr);

        let mut slots = Vec::new(&env);
        slots.push_back((winner.clone(), bounty.amount));

        escrow_client.define_release_slots(&pool_id, &slots);
        escrow_client.release_slot(&pool_id, &0);

        bounty.status = BountyStatus::Completed;
        bounty.assignee = Some(winner.clone());
        env.storage().persistent().set(&key, &bounty);

        SubmissionAccepted {
            bounty_id,
            assignee: winner.clone(),
        }
        .publish(&env);

        let amount_scaled = if bounty.amount > 0 {
            bounty.amount as u64
        } else {
            0
        };
        let points = (amount_scaled * (rating as u64)) / 1000;
        let points_u32 = if points > 10000 { 10000 } else { points as u32 };

        let rep_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::ReputationRegistry)
            .ok_or(Error::NotInitialized)?;
        let rep_client = ReputationRegistryClient::new(&env, &rep_addr);

        rep_client.record_completion(
            &env.current_contract_address(),
            &winner,
            &bounty_id,
            &bounty.category,
            &points_u32,
            &false,
            &true,
        );
        Ok(())
    }

    pub fn cancel_bounty(env: Env, creator: Address, bounty_id: u64) -> Result<(), Error> {
        creator.require_auth();

        let key = DataKey::Bounty(bounty_id);
        let mut bounty: Bounty = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::BountyNotFound)?;

        if bounty.creator != creator {
            return Err(Error::NotCreator);
        }
        if bounty.status != BountyStatus::Open {
            return Err(Error::BountyNotOpen);
        }

        bounty.status = BountyStatus::Cancelled;
        env.storage().persistent().set(&key, &bounty);

        let core_escrow_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;
        let escrow_client = CoreEscrowClient::new(&env, &core_escrow_addr);
        let pool_id = bounty.escrow_pool_id.ok_or(Error::NoEscrowPool)?;
        escrow_client.refund_all(&pool_id);

        BountyCancelled { bounty_id }.publish(&env);
        Ok(())
    }

    pub fn reject_application(
        env: Env,
        creator: Address,
        bounty_id: u64,
        applicant: Address,
    ) -> Result<(), Error> {
        creator.require_auth();

        let key = DataKey::Bounty(bounty_id);
        let bounty: Bounty = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::BountyNotFound)?;

        if bounty.creator != creator {
            return Err(Error::NotCreator);
        }

        let app_key = DataKey::Application(bounty_id, applicant.clone());
        let mut app: Application = env
            .storage()
            .persistent()
            .get(&app_key)
            .ok_or(Error::ApplicationNotFound)?;

        if app.status != ApplicationStatus::Pending {
            return Err(Error::ApplicationNotPending);
        }

        app.status = ApplicationStatus::Rejected;
        env.storage().persistent().set(&app_key, &app);

        ApplicationRejected {
            bounty_id,
            applicant,
        }
        .publish(&env);
        Ok(())
    }

    pub fn update_bounty(
        env: Env,
        creator: Address,
        bounty_id: u64,
        title: Option<String>,
        metadata_cid: Option<String>,
        deadline: Option<u64>,
    ) -> Result<(), Error> {
        creator.require_auth();

        let key = DataKey::Bounty(bounty_id);
        let mut bounty: Bounty = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::BountyNotFound)?;

        if bounty.creator != creator {
            return Err(Error::NotCreator);
        }
        if bounty.status != BountyStatus::Open {
            return Err(Error::BountyNotOpen);
        }

        if let Some(t) = title {
            bounty.title = t;
        }
        if let Some(m) = metadata_cid {
            bounty.metadata_cid = m;
        }
        if let Some(d) = deadline {
            if d <= env.ledger().timestamp() {
                return Err(Error::BountyDeadlinePassed);
            }
            bounty.deadline = d;
        }

        env.storage().persistent().set(&key, &bounty);
        Ok(())
    }
}
