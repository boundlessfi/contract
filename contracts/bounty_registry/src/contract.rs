use crate::error::Error;
use crate::events::{
    ApplicationRejected, BountyApplied, BountyCancelled, BountyClaimed, BountyAssigned,
    BountyCreated, SplitApproved, SubmissionApproved, WorkSubmitted,
};
use crate::storage::{
    Application, ApplicationStatus, Bounty, BountyStatus, BountyType, DataKey,
};
use boundless_types::{ActivityCategory, ModuleType};
use core_escrow::CoreEscrowClient;
use reputation_registry::ReputationRegistryClient;
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

#[contract]
pub struct BountyRegistry;

#[contractimpl]
impl BountyRegistry {
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
        env.storage().instance().set(&DataKey::BountyCount, &0u64);
        Ok(())
    }

    // ========================================================================
    // QUERIES
    // ========================================================================

    pub fn get_bounty(env: Env, bounty_id: u64) -> Result<Bounty, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Bounty(bounty_id))
            .ok_or(Error::BountyNotFound)
    }

    pub fn get_application(
        env: Env,
        bounty_id: u64,
        applicant: Address,
    ) -> Result<Application, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Application(bounty_id, applicant))
            .ok_or(Error::ApplicationNotFound)
    }

    pub fn get_bounty_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::BountyCount)
            .unwrap_or(0)
    }

    // ========================================================================
    // BOUNTY CREATION
    // ========================================================================

    pub fn create_bounty(
        env: Env,
        creator: Address,
        title: String,
        metadata_cid: String,
        bounty_type: BountyType,
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
            return Err(Error::DeadlinePassed);
        }

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::BountyCount)
            .unwrap_or(0);
        count += 1;
        env.storage().instance().set(&DataKey::BountyCount, &count);

        let escrow_client = Self::escrow_client(&env);

        let pool_id = escrow_client.create_pool(
            &creator,
            &ModuleType::Bounty,
            &count,
            &amount,
            &asset,
            &deadline,
            &env.current_contract_address(),
        );

        // Contest and Split bounties lock the pool immediately
        if bounty_type == BountyType::Contest || bounty_type == BountyType::Split {
            escrow_client.lock_pool(&pool_id);
        }

        let bounty = Bounty {
            id: count,
            creator: creator.clone(),
            title,
            metadata_cid,
            bounty_type,
            status: BountyStatus::Open,
            amount,
            asset,
            category,
            created_at: env.ledger().timestamp(),
            deadline,
            assignee: None,
            escrow_pool_id: pool_id,
            winner_count: 0,
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

    // ========================================================================
    // FCFS FLOW: claim → approve
    // ========================================================================

    /// FCFS only: first contributor to claim gets the bounty.
    /// Spends 1 SparkCredit, locks escrow, assigns contributor.
    pub fn claim_bounty(env: Env, contributor: Address, bounty_id: u64) -> Result<(), Error> {
        contributor.require_auth();

        let key = DataKey::Bounty(bounty_id);
        let mut bounty: Bounty = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::BountyNotFound)?;

        if bounty.bounty_type != BountyType::FCFS {
            return Err(Error::InvalidSubType);
        }
        if bounty.status != BountyStatus::Open {
            return Err(Error::BountyNotOpen);
        }
        if env.ledger().timestamp() > bounty.deadline {
            return Err(Error::DeadlinePassed);
        }

        // Spend 1 SparkCredit
        let rep_client = Self::rep_client(&env);
        let had_credit =
            rep_client.spend_credit(&env.current_contract_address(), &contributor);
        if !had_credit {
            return Err(Error::InsufficientCredits);
        }

        // Lock escrow and define single release slot
        let escrow_client = Self::escrow_client(&env);
        escrow_client.lock_pool(&bounty.escrow_pool_id);

        let mut slots = Vec::new(&env);
        slots.push_back((contributor.clone(), bounty.amount));
        escrow_client.define_release_slots(&bounty.escrow_pool_id, &slots);

        bounty.assignee = Some(contributor.clone());
        bounty.status = BountyStatus::InProgress;
        env.storage().persistent().set(&key, &bounty);

        BountyClaimed {
            bounty_id,
            claimer: contributor,
        }
        .publish(&env);

        Ok(())
    }

    /// FCFS: creator approves the claimed work and releases payment.
    pub fn approve_fcfs(
        env: Env,
        creator: Address,
        bounty_id: u64,
        points: u32,
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
        if bounty.bounty_type != BountyType::FCFS {
            return Err(Error::InvalidSubType);
        }
        if bounty.status != BountyStatus::InProgress {
            return Err(Error::NotInProgress);
        }

        let winner = bounty.assignee.clone().ok_or(Error::NotAssignee)?;

        // Release escrow
        let escrow_client = Self::escrow_client(&env);
        escrow_client.release_slot(&bounty.escrow_pool_id, &0);

        // Record reputation
        let rep_client = Self::rep_client(&env);
        rep_client.record_completion(
            &env.current_contract_address(),
            &winner,
            &bounty.category,
            &points,
        );

        // Award bonus credit for completion
        rep_client.award_credits(&env.current_contract_address(), &winner, &1);

        bounty.status = BountyStatus::Completed;
        bounty.winner_count = 1;
        env.storage().persistent().set(&key, &bounty);

        SubmissionApproved {
            bounty_id,
            winner,
        }
        .publish(&env);

        Ok(())
    }

    // ========================================================================
    // APPLICATION FLOW: apply → select → submit → approve
    // ========================================================================

    /// Application type: applicant submits a proposal. Spends 1 SparkCredit.
    pub fn apply(
        env: Env,
        applicant: Address,
        bounty_id: u64,
        proposal: String,
    ) -> Result<(), Error> {
        applicant.require_auth();

        let bounty: Bounty = env
            .storage()
            .persistent()
            .get(&DataKey::Bounty(bounty_id))
            .ok_or(Error::BountyNotFound)?;

        if bounty.bounty_type != BountyType::Application {
            return Err(Error::InvalidSubType);
        }
        if bounty.status != BountyStatus::Open {
            return Err(Error::BountyNotOpen);
        }
        if env.ledger().timestamp() > bounty.deadline {
            return Err(Error::DeadlinePassed);
        }

        let app_key = DataKey::Application(bounty_id, applicant.clone());
        if env.storage().persistent().has(&app_key) {
            return Err(Error::AlreadyApplied);
        }

        // Spend 1 SparkCredit
        let rep_client = Self::rep_client(&env);
        let had_credit =
            rep_client.spend_credit(&env.current_contract_address(), &applicant);
        if !had_credit {
            return Err(Error::InsufficientCredits);
        }

        let app = Application {
            bounty_id,
            applicant: applicant.clone(),
            proposal,
            submitted_at: env.ledger().timestamp(),
            status: ApplicationStatus::Pending,
        };
        env.storage().persistent().set(&app_key, &app);

        // Track applicant index for credit restoration later
        let app_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ApplicantCount(bounty_id))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::Applicant(bounty_id, app_count), &applicant);
        env.storage()
            .persistent()
            .set(&DataKey::ApplicantCount(bounty_id), &(app_count + 1));

        BountyApplied {
            bounty_id,
            applicant,
        }
        .publish(&env);

        Ok(())
    }

    /// Creator selects an applicant. Locks escrow, restores credits to non-selected.
    pub fn select_applicant(
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
        if bounty.bounty_type != BountyType::Application {
            return Err(Error::InvalidSubType);
        }
        if bounty.status != BountyStatus::Open {
            return Err(Error::BountyNotOpen);
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

        app.status = ApplicationStatus::Accepted;
        env.storage().persistent().set(&app_key, &app);

        // Lock escrow and define release slot for selected applicant
        let escrow_client = Self::escrow_client(&env);
        escrow_client.lock_pool(&bounty.escrow_pool_id);

        let mut slots = Vec::new(&env);
        slots.push_back((applicant.clone(), bounty.amount));
        escrow_client.define_release_slots(&bounty.escrow_pool_id, &slots);

        // Restore credits to non-selected applicants
        let rep_client = Self::rep_client(&env);
        let app_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ApplicantCount(bounty_id))
            .unwrap_or(0);
        let contract_addr = env.current_contract_address();
        for i in 0..app_count {
            if let Some(addr) = env
                .storage()
                .persistent()
                .get::<_, Address>(&DataKey::Applicant(bounty_id, i))
            {
                if addr != applicant {
                    rep_client.restore_credit(&contract_addr, &addr);
                    // Mark rejected
                    let other_key = DataKey::Application(bounty_id, addr.clone());
                    if let Some(mut other_app) = env
                        .storage()
                        .persistent()
                        .get::<_, Application>(&other_key)
                    {
                        other_app.status = ApplicationStatus::Rejected;
                        env.storage().persistent().set(&other_key, &other_app);
                    }
                }
            }
        }

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

    /// Submit work for Application or Contest bounties.
    pub fn submit_work(
        env: Env,
        contributor: Address,
        bounty_id: u64,
        work_cid: String,
    ) -> Result<(), Error> {
        contributor.require_auth();

        let key = DataKey::Bounty(bounty_id);
        let mut bounty: Bounty = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::BountyNotFound)?;

        if env.ledger().timestamp() > bounty.deadline {
            return Err(Error::DeadlinePassed);
        }

        match bounty.bounty_type {
            BountyType::Application => {
                // Only the assigned contributor can submit
                if bounty.assignee != Some(contributor.clone()) {
                    return Err(Error::NotAssignee);
                }
                if bounty.status != BountyStatus::InProgress {
                    return Err(Error::NotInProgress);
                }
                bounty.status = BountyStatus::InReview;
            }
            BountyType::Contest => {
                // Anyone can submit to a contest (store as application)
                if bounty.status != BountyStatus::Open {
                    return Err(Error::BountyNotOpen);
                }
                let app_key = DataKey::Application(bounty_id, contributor.clone());
                if env.storage().persistent().has(&app_key) {
                    return Err(Error::AlreadyApplied);
                }
                let app = Application {
                    bounty_id,
                    applicant: contributor.clone(),
                    proposal: work_cid.clone(),
                    submitted_at: env.ledger().timestamp(),
                    status: ApplicationStatus::Pending,
                };
                env.storage().persistent().set(&app_key, &app);
            }
            _ => {
                return Err(Error::InvalidSubType);
            }
        }

        env.storage().persistent().set(&key, &bounty);

        WorkSubmitted {
            bounty_id,
            contributor,
        }
        .publish(&env);

        Ok(())
    }

    /// Creator approves a submission for Application bounties.
    pub fn approve_submission(
        env: Env,
        creator: Address,
        bounty_id: u64,
        points: u32,
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
        if bounty.bounty_type != BountyType::Application {
            return Err(Error::InvalidSubType);
        }
        if bounty.status != BountyStatus::InReview {
            return Err(Error::NotReviewable);
        }

        let winner = bounty.assignee.clone().ok_or(Error::NotAssignee)?;

        // Release escrow
        let escrow_client = Self::escrow_client(&env);
        escrow_client.release_slot(&bounty.escrow_pool_id, &0);

        // Record reputation + award credit
        let rep_client = Self::rep_client(&env);
        let contract_addr = env.current_contract_address();
        rep_client.record_completion(&contract_addr, &winner, &bounty.category, &points);
        rep_client.award_credits(&contract_addr, &winner, &1);

        bounty.status = BountyStatus::Completed;
        bounty.winner_count = 1;
        env.storage().persistent().set(&key, &bounty);

        SubmissionApproved {
            bounty_id,
            winner,
        }
        .publish(&env);

        Ok(())
    }

    // ========================================================================
    // CONTEST FLOW: submit_work → approve_contest_winner (per winner)
    // ========================================================================

    /// Contest: creator approves a winning submission and releases a share.
    pub fn approve_contest_winner(
        env: Env,
        creator: Address,
        bounty_id: u64,
        winner: Address,
        payout_amount: i128,
        points: u32,
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
        if bounty.bounty_type != BountyType::Contest {
            return Err(Error::NotContestType);
        }
        if bounty.status != BountyStatus::Open && bounty.status != BountyStatus::InProgress {
            return Err(Error::BountyNotOpen);
        }

        // Verify submission exists
        let app_key = DataKey::Application(bounty_id, winner.clone());
        if !env.storage().persistent().has(&app_key) {
            return Err(Error::ApplicationNotFound);
        }

        // Release partial from escrow
        let escrow_client = Self::escrow_client(&env);
        escrow_client.release_partial(
            &bounty.escrow_pool_id,
            &winner,
            &payout_amount,
        );

        // Record reputation + award credit
        let rep_client = Self::rep_client(&env);
        let contract_addr = env.current_contract_address();
        rep_client.record_completion(&contract_addr, &winner, &bounty.category, &points);
        rep_client.award_credits(&contract_addr, &winner, &1);

        bounty.winner_count += 1;
        bounty.status = BountyStatus::InProgress;
        env.storage().persistent().set(&key, &bounty);

        SubmissionApproved {
            bounty_id,
            winner,
        }
        .publish(&env);

        Ok(())
    }

    /// Contest: finalize after all winners approved. Refunds remaining escrow.
    pub fn finalize_contest(
        env: Env,
        creator: Address,
        bounty_id: u64,
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
        if bounty.bounty_type != BountyType::Contest {
            return Err(Error::NotContestType);
        }

        // Refund any remaining escrow to creator
        let escrow_client = Self::escrow_client(&env);
        escrow_client.refund_remaining(&bounty.escrow_pool_id);

        bounty.status = BountyStatus::Completed;
        env.storage().persistent().set(&key, &bounty);

        Ok(())
    }

    // ========================================================================
    // SPLIT FLOW: define_splits → approve_split (per slot)
    // ========================================================================

    /// Split: creator defines how the bounty is divided among contributors.
    pub fn define_splits(
        env: Env,
        creator: Address,
        bounty_id: u64,
        slots: Vec<(Address, i128)>,
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
        if bounty.bounty_type != BountyType::Split {
            return Err(Error::NotSplitType);
        }
        if bounty.status != BountyStatus::Open {
            return Err(Error::BountyNotOpen);
        }

        // Validate total doesn't exceed bounty amount
        let mut total: i128 = 0;
        for (_, amount) in slots.iter() {
            total += amount;
        }
        if total > bounty.amount {
            return Err(Error::InvalidSplitShares);
        }

        let escrow_client = Self::escrow_client(&env);
        escrow_client.define_release_slots(&bounty.escrow_pool_id, &slots);

        Ok(())
    }

    /// Split: creator approves a specific contributor's milestone/slot.
    pub fn approve_split(
        env: Env,
        creator: Address,
        bounty_id: u64,
        slot_index: u32,
        points: u32,
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
        if bounty.bounty_type != BountyType::Split {
            return Err(Error::NotSplitType);
        }

        // Get slot to find the recipient
        let escrow_client = Self::escrow_client(&env);
        let slot = escrow_client.get_slot(&bounty.escrow_pool_id, &slot_index);

        // Release the slot
        escrow_client.release_slot(&bounty.escrow_pool_id, &slot_index);

        // Record reputation for the recipient
        let rep_client = Self::rep_client(&env);
        let contract_addr = env.current_contract_address();
        rep_client.record_completion(&contract_addr, &slot.recipient, &bounty.category, &points);
        rep_client.award_credits(&contract_addr, &slot.recipient, &1);

        bounty.winner_count += 1;
        bounty.status = BountyStatus::InProgress;
        env.storage().persistent().set(&key, &bounty);

        SplitApproved {
            bounty_id,
            slot_index,
        }
        .publish(&env);

        Ok(())
    }

    // ========================================================================
    // COMMON OPERATIONS
    // ========================================================================

    /// Cancel a bounty. Only if Open (no active claims/selections).
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
            return Err(Error::CannotCancel);
        }

        // Restore credits to all applicants
        let rep_client = Self::rep_client(&env);
        let contract_addr = env.current_contract_address();
        let app_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::ApplicantCount(bounty_id))
            .unwrap_or(0);
        for i in 0..app_count {
            if let Some(addr) = env
                .storage()
                .persistent()
                .get::<_, Address>(&DataKey::Applicant(bounty_id, i))
            {
                rep_client.restore_credit(&contract_addr, &addr);
            }
        }

        // Refund escrow
        let escrow_client = Self::escrow_client(&env);
        escrow_client.refund_all(&bounty.escrow_pool_id);

        bounty.status = BountyStatus::Cancelled;
        env.storage().persistent().set(&key, &bounty);

        BountyCancelled { bounty_id }.publish(&env);
        Ok(())
    }

    /// Reject an application. Restores the applicant's SparkCredit.
    pub fn reject_application(
        env: Env,
        creator: Address,
        bounty_id: u64,
        applicant: Address,
    ) -> Result<(), Error> {
        creator.require_auth();

        let bounty: Bounty = env
            .storage()
            .persistent()
            .get(&DataKey::Bounty(bounty_id))
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

        // Restore SparkCredit
        let rep_client = Self::rep_client(&env);
        rep_client.restore_credit(&env.current_contract_address(), &applicant);

        ApplicationRejected {
            bounty_id,
            applicant,
        }
        .publish(&env);

        Ok(())
    }

    /// Update bounty metadata (only while Open).
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
                return Err(Error::DeadlinePassed);
            }
            bounty.deadline = d;
        }

        env.storage().persistent().set(&key, &bounty);
        Ok(())
    }

    // ========================================================================
    // ADMIN
    // ========================================================================

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    // ========================================================================
    // INTERNAL HELPERS
    // ========================================================================

    fn escrow_client(env: &Env) -> CoreEscrowClient<'_> {
        let addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .unwrap();
        CoreEscrowClient::new(env, &addr)
    }

    fn rep_client(env: &Env) -> ReputationRegistryClient<'_> {
        let addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::ReputationRegistry)
            .unwrap();
        ReputationRegistryClient::new(env, &addr)
    }
}
