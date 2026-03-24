use crate::error::Error;
use crate::events::{
    ApplicationRejected, BountyApplied, BountyAssigned, BountyCancelled, BountyClaimed,
    BountyCreated, SplitApproved, SubmissionApproved, WorkSubmitted,
};
use crate::storage::{Application, ApplicationStatus, Bounty, BountyStatus, BountyType, DataKey};
use boundless_types::ttl::{
    INSTANCE_TTL_EXTEND, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD,
};
use boundless_types::{ActivityCategory, ModuleType};
use soroban_sdk::{
    contract, contractimpl, Address, BytesN, Env, IntoVal, String, Symbol, Val, Vec,
};

fn sym(env: &Env, name: &str) -> Symbol {
    Symbol::new(env, name)
}

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
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    // ========================================================================
    // QUERIES
    // ========================================================================

    pub fn get_bounty(env: Env, bounty_id: u64) -> Result<Bounty, Error> {
        let key = DataKey::Bounty(bounty_id);
        let bounty = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::BountyNotFound)?;
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);
        Ok(bounty)
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

        let escrow_addr = Self::get_escrow_addr(&env)?;

        let pool_args: Vec<Val> = Vec::from_array(
            &env,
            [
                creator.clone().into_val(&env),
                ModuleType::Bounty.into_val(&env),
                count.into_val(&env),
                amount.into_val(&env),
                asset.clone().into_val(&env),
                deadline.into_val(&env),
                env.current_contract_address().into_val(&env),
            ],
        );
        let pool_id: BytesN<32> =
            env.invoke_contract(&escrow_addr, &sym(&env, "create_pool"), pool_args);

        // Contest and Split bounties lock the pool immediately
        if bounty_type == BountyType::Contest || bounty_type == BountyType::Split {
            let lock_args: Vec<Val> = Vec::from_array(&env, [pool_id.clone().into_val(&env)]);
            env.invoke_contract::<()>(&escrow_addr, &sym(&env, "lock_pool"), lock_args);
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

        let key = DataKey::Bounty(count);
        env.storage().persistent().set(&key, &bounty);
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);

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
        let rep_addr = Self::get_rep_addr(&env)?;
        let contract_addr = env.current_contract_address();
        let spend_args: Vec<Val> = Vec::from_array(
            &env,
            [
                contract_addr.into_val(&env),
                contributor.clone().into_val(&env),
            ],
        );
        let had_credit: bool =
            env.invoke_contract(&rep_addr, &sym(&env, "spend_credit"), spend_args);
        if !had_credit {
            return Err(Error::InsufficientCredits);
        }

        // Lock escrow and define single release slot
        let escrow_addr = Self::get_escrow_addr(&env)?;
        let lock_args: Vec<Val> =
            Vec::from_array(&env, [bounty.escrow_pool_id.clone().into_val(&env)]);
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "lock_pool"), lock_args);

        let mut slots: Vec<(Address, i128)> = Vec::new(&env);
        slots.push_back((contributor.clone(), bounty.amount));
        let slot_args: Vec<Val> = Vec::from_array(
            &env,
            [
                bounty.escrow_pool_id.clone().into_val(&env),
                slots.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "define_release_slots"), slot_args);

        bounty.assignee = Some(contributor.clone());
        bounty.status = BountyStatus::InProgress;
        env.storage().persistent().set(&key, &bounty);
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);

        BountyClaimed {
            bounty_id,
            claimer: contributor,
        }
        .publish(&env);

        Ok(())
    }

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
        let escrow_addr = Self::get_escrow_addr(&env)?;
        let release_args: Vec<Val> = Vec::from_array(
            &env,
            [
                bounty.escrow_pool_id.clone().into_val(&env),
                0u32.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "release_slot"), release_args);

        // Record reputation + award credit
        let rep_addr = Self::get_rep_addr(&env)?;
        let contract_addr = env.current_contract_address();
        let comp_args: Vec<Val> = Vec::from_array(
            &env,
            [
                contract_addr.clone().into_val(&env),
                winner.clone().into_val(&env),
                bounty.category.into_val(&env),
                points.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&rep_addr, &sym(&env, "record_completion"), comp_args);

        let award_args: Vec<Val> = Vec::from_array(
            &env,
            [
                contract_addr.into_val(&env),
                winner.clone().into_val(&env),
                1u32.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&rep_addr, &sym(&env, "award_credits"), award_args);

        bounty.status = BountyStatus::Completed;
        bounty.winner_count = 1;
        env.storage().persistent().set(&key, &bounty);

        SubmissionApproved { bounty_id, winner }.publish(&env);

        Ok(())
    }

    // ========================================================================
    // APPLICATION FLOW: apply → select → submit → approve
    // ========================================================================

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
        let rep_addr = Self::get_rep_addr(&env)?;
        let contract_addr = env.current_contract_address();
        let spend_args: Vec<Val> = Vec::from_array(
            &env,
            [
                contract_addr.into_val(&env),
                applicant.clone().into_val(&env),
            ],
        );
        let had_credit: bool =
            env.invoke_contract(&rep_addr, &sym(&env, "spend_credit"), spend_args);
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
        Self::extend_persistent_ttl(&env, &app_key);
        Self::extend_instance_ttl(&env);

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

        // Lock escrow and define release slot
        let escrow_addr = Self::get_escrow_addr(&env)?;
        let lock_args: Vec<Val> =
            Vec::from_array(&env, [bounty.escrow_pool_id.clone().into_val(&env)]);
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "lock_pool"), lock_args);

        let mut slots: Vec<(Address, i128)> = Vec::new(&env);
        slots.push_back((applicant.clone(), bounty.amount));
        let slot_args: Vec<Val> = Vec::from_array(
            &env,
            [
                bounty.escrow_pool_id.clone().into_val(&env),
                slots.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "define_release_slots"), slot_args);

        // Restore credits to non-selected applicants
        let rep_addr = Self::get_rep_addr(&env)?;
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
                if addr != applicant {
                    let restore_args: Vec<Val> = Vec::from_array(
                        &env,
                        [
                            contract_addr.clone().into_val(&env),
                            addr.clone().into_val(&env),
                        ],
                    );
                    env.invoke_contract::<()>(
                        &rep_addr,
                        &sym(&env, "restore_credit"),
                        restore_args,
                    );
                    // Mark rejected
                    let other_key = DataKey::Application(bounty_id, addr);
                    if let Some(mut other_app) =
                        env.storage().persistent().get::<_, Application>(&other_key)
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
                if bounty.assignee != Some(contributor.clone()) {
                    return Err(Error::NotAssignee);
                }
                if bounty.status != BountyStatus::InProgress {
                    return Err(Error::NotInProgress);
                }
                bounty.status = BountyStatus::InReview;
            }
            BountyType::Contest => {
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
        let escrow_addr = Self::get_escrow_addr(&env)?;
        let release_args: Vec<Val> = Vec::from_array(
            &env,
            [
                bounty.escrow_pool_id.clone().into_val(&env),
                0u32.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "release_slot"), release_args);

        // Record reputation + award credit
        let rep_addr = Self::get_rep_addr(&env)?;
        let contract_addr = env.current_contract_address();
        let comp_args: Vec<Val> = Vec::from_array(
            &env,
            [
                contract_addr.clone().into_val(&env),
                winner.clone().into_val(&env),
                bounty.category.into_val(&env),
                points.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&rep_addr, &sym(&env, "record_completion"), comp_args);

        let award_args: Vec<Val> = Vec::from_array(
            &env,
            [
                contract_addr.into_val(&env),
                winner.clone().into_val(&env),
                1u32.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&rep_addr, &sym(&env, "award_credits"), award_args);

        bounty.status = BountyStatus::Completed;
        bounty.winner_count = 1;
        env.storage().persistent().set(&key, &bounty);

        SubmissionApproved { bounty_id, winner }.publish(&env);

        Ok(())
    }

    // ========================================================================
    // CONTEST FLOW: submit_work → approve_contest_winner (per winner)
    // ========================================================================

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
        let escrow_addr = Self::get_escrow_addr(&env)?;
        let release_args: Vec<Val> = Vec::from_array(
            &env,
            [
                bounty.escrow_pool_id.clone().into_val(&env),
                winner.clone().into_val(&env),
                payout_amount.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "release_partial"), release_args);

        // Record reputation + award credit
        let rep_addr = Self::get_rep_addr(&env)?;
        let contract_addr = env.current_contract_address();
        let comp_args: Vec<Val> = Vec::from_array(
            &env,
            [
                contract_addr.clone().into_val(&env),
                winner.clone().into_val(&env),
                bounty.category.into_val(&env),
                points.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&rep_addr, &sym(&env, "record_completion"), comp_args);

        let award_args: Vec<Val> = Vec::from_array(
            &env,
            [
                contract_addr.into_val(&env),
                winner.clone().into_val(&env),
                1u32.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&rep_addr, &sym(&env, "award_credits"), award_args);

        bounty.winner_count += 1;
        bounty.status = BountyStatus::InProgress;
        env.storage().persistent().set(&key, &bounty);

        SubmissionApproved { bounty_id, winner }.publish(&env);

        Ok(())
    }

    pub fn finalize_contest(env: Env, creator: Address, bounty_id: u64) -> Result<(), Error> {
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

        // Refund remaining escrow to creator
        let escrow_addr = Self::get_escrow_addr(&env)?;
        let refund_args: Vec<Val> =
            Vec::from_array(&env, [bounty.escrow_pool_id.clone().into_val(&env)]);
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "refund_remaining"), refund_args);

        bounty.status = BountyStatus::Completed;
        env.storage().persistent().set(&key, &bounty);

        Ok(())
    }

    // ========================================================================
    // SPLIT FLOW: define_splits → approve_split (per slot)
    // ========================================================================

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

        // Store recipients locally for later use in approve_split
        for (i, (addr, _)) in slots.iter().enumerate() {
            env.storage()
                .persistent()
                .set(&DataKey::SplitRecipient(bounty_id, i as u32), &addr);
        }

        let escrow_addr = Self::get_escrow_addr(&env)?;
        let slot_args: Vec<Val> = Vec::from_array(
            &env,
            [
                bounty.escrow_pool_id.clone().into_val(&env),
                slots.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "define_release_slots"), slot_args);

        Ok(())
    }

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

        // Get locally stored recipient
        let recipient: Address = env
            .storage()
            .persistent()
            .get(&DataKey::SplitRecipient(bounty_id, slot_index))
            .ok_or(Error::ApplicationNotFound)?;

        // Release the slot
        let escrow_addr = Self::get_escrow_addr(&env)?;
        let release_args: Vec<Val> = Vec::from_array(
            &env,
            [
                bounty.escrow_pool_id.clone().into_val(&env),
                slot_index.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "release_slot"), release_args);

        // Record reputation for the recipient
        let rep_addr = Self::get_rep_addr(&env)?;
        let contract_addr = env.current_contract_address();
        let comp_args: Vec<Val> = Vec::from_array(
            &env,
            [
                contract_addr.clone().into_val(&env),
                recipient.clone().into_val(&env),
                bounty.category.into_val(&env),
                points.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&rep_addr, &sym(&env, "record_completion"), comp_args);

        let award_args: Vec<Val> = Vec::from_array(
            &env,
            [
                contract_addr.into_val(&env),
                recipient.into_val(&env),
                1u32.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&rep_addr, &sym(&env, "award_credits"), award_args);

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
    // FCFS AUTO-RELEASE
    // ========================================================================

    pub fn auto_release_check(env: Env, bounty_id: u64) -> Result<(), Error> {
        let key = DataKey::Bounty(bounty_id);
        let mut bounty: Bounty = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::BountyNotFound)?;

        if bounty.bounty_type != BountyType::FCFS {
            return Err(Error::NotFCFSType);
        }
        if bounty.status != BountyStatus::InProgress {
            return Err(Error::NotInProgress);
        }

        // 7 days = 604_800 seconds after the deadline
        if env.ledger().timestamp() <= bounty.deadline + 604_800 {
            return Err(Error::AutoReleaseNotReady);
        }

        let winner = bounty.assignee.clone().ok_or(Error::NotAssignee)?;

        // Release escrow slot 0
        let escrow_addr = Self::get_escrow_addr(&env)?;
        let release_args: Vec<Val> = Vec::from_array(
            &env,
            [
                bounty.escrow_pool_id.clone().into_val(&env),
                0u32.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "release_slot"), release_args);

        // Record reputation with default 50 points for auto-release
        let rep_addr = Self::get_rep_addr(&env)?;
        let contract_addr = env.current_contract_address();
        let comp_args: Vec<Val> = Vec::from_array(
            &env,
            [
                contract_addr.clone().into_val(&env),
                winner.clone().into_val(&env),
                bounty.category.into_val(&env),
                50u32.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&rep_addr, &sym(&env, "record_completion"), comp_args);

        // Award 1 credit
        let award_args: Vec<Val> = Vec::from_array(
            &env,
            [
                contract_addr.into_val(&env),
                winner.into_val(&env),
                1u32.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&rep_addr, &sym(&env, "award_credits"), award_args);

        bounty.status = BountyStatus::Completed;
        bounty.winner_count = 1;
        env.storage().persistent().set(&key, &bounty);
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);

        Ok(())
    }

    // ========================================================================
    // COMMON OPERATIONS
    // ========================================================================

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
        let rep_addr = Self::get_rep_addr(&env)?;
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
                let restore_args: Vec<Val> = Vec::from_array(
                    &env,
                    [contract_addr.clone().into_val(&env), addr.into_val(&env)],
                );
                env.invoke_contract::<()>(&rep_addr, &sym(&env, "restore_credit"), restore_args);
            }
        }

        // Refund escrow
        let escrow_addr = Self::get_escrow_addr(&env)?;
        let refund_args: Vec<Val> =
            Vec::from_array(&env, [bounty.escrow_pool_id.clone().into_val(&env)]);
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "refund_all"), refund_args);

        bounty.status = BountyStatus::Cancelled;
        env.storage().persistent().set(&key, &bounty);

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
        let rep_addr = Self::get_rep_addr(&env)?;
        let restore_args: Vec<Val> = Vec::from_array(
            &env,
            [
                env.current_contract_address().into_val(&env),
                applicant.clone().into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&rep_addr, &sym(&env, "restore_credit"), restore_args);

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
