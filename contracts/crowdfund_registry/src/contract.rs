use crate::error::CrowdfundError;
use crate::events::{
    CampaignApproved, CampaignCancelled, CampaignCreated, CampaignFailed, CampaignFunded,
    CampaignRejected, CampaignSubmittedForReview, CampaignTerminated, CampaignValidated,
    CampaignVoteRejected, DisputeResolved, MilestoneApproved, MilestoneDisputed, MilestoneOverdue,
    MilestoneRevisionRequested, MilestoneSubmitted, PledgeRecorded, RefundBatchProcessed,
};
use crate::storage::{
    Campaign, CampaignStatus, CrowdfundDataKey, CrowdfundMilestoneStatus, DisputeResolution,
    Milestone, VoteContext, VoteOption, VotingSession,
};
use boundless_types::ttl::{
    INSTANCE_TTL_EXTEND, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD,
};
use boundless_types::ModuleType;
use soroban_sdk::{
    contract, contractimpl, Address, BytesN, Env, IntoVal, String, Symbol, Val, Vec,
};

const BACKER_BATCH_SIZE: u32 = 50;

fn sym(env: &Env, name: &str) -> Symbol {
    Symbol::new(env, name)
}

#[contract]
pub struct CrowdfundRegistry;

#[contractimpl]
impl CrowdfundRegistry {
    // ========================================================================
    // INITIALIZATION
    // ========================================================================

    pub fn init(
        env: Env,
        admin: Address,
        core_escrow: Address,
        reputation_registry: Address,
        governance_voting: Address,
    ) -> Result<(), CrowdfundError> {
        if env.storage().instance().has(&CrowdfundDataKey::Admin) {
            return Err(CrowdfundError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage()
            .instance()
            .set(&CrowdfundDataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&CrowdfundDataKey::CoreEscrow, &core_escrow);
        env.storage()
            .instance()
            .set(&CrowdfundDataKey::ReputationRegistry, &reputation_registry);
        env.storage()
            .instance()
            .set(&CrowdfundDataKey::GovernanceVoting, &governance_voting);
        env.storage()
            .instance()
            .set(&CrowdfundDataKey::CampaignCount, &0u64);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    // ========================================================================
    // QUERIES
    // ========================================================================

    pub fn get_campaign(env: Env, campaign_id: u64) -> Result<Campaign, CrowdfundError> {
        let key = CrowdfundDataKey::Campaign(campaign_id);
        let campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);
        Ok(campaign)
    }

    pub fn get_milestone(
        env: Env,
        campaign_id: u64,
        milestone_index: u32,
    ) -> Result<Milestone, CrowdfundError> {
        env.storage()
            .persistent()
            .get(&CrowdfundDataKey::CampaignMilestone(
                campaign_id,
                milestone_index,
            ))
            .ok_or(CrowdfundError::MilestoneNotFound)
    }

    pub fn get_dispute_status(
        env: Env,
        campaign_id: u64,
        milestone_index: u32,
    ) -> Result<CrowdfundMilestoneStatus, CrowdfundError> {
        let ms_key = CrowdfundDataKey::CampaignMilestone(campaign_id, milestone_index);
        let ms: Milestone = env
            .storage()
            .persistent()
            .get(&ms_key)
            .ok_or(CrowdfundError::MilestoneNotFound)?;
        Ok(ms.status)
    }

    pub fn get_pledge(env: Env, campaign_id: u64, backer: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&CrowdfundDataKey::Pledge(campaign_id, backer))
            .unwrap_or(0)
    }

    pub fn get_campaign_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&CrowdfundDataKey::CampaignCount)
            .unwrap_or(0)
    }

    pub fn get_vote_session(env: Env, campaign_id: u64) -> Result<BytesN<32>, CrowdfundError> {
        let campaign: Campaign = env
            .storage()
            .persistent()
            .get(&CrowdfundDataKey::Campaign(campaign_id))
            .ok_or(CrowdfundError::CampaignNotFound)?;
        campaign
            .vote_session_id
            .ok_or(CrowdfundError::NoVoteSession)
    }

    // ========================================================================
    // CAMPAIGN CREATION (starts in Draft)
    // ========================================================================

    pub fn create_campaign(
        env: Env,
        owner: Address,
        metadata_cid: String,
        funding_goal: i128,
        asset: Address,
        deadline: u64,
        milestone_descs: Vec<(String, u32)>,
        min_pledge: i128,
        submit: bool,
    ) -> Result<u64, CrowdfundError> {
        owner.require_auth();

        if funding_goal <= 0 {
            return Err(CrowdfundError::AmountNotPositive);
        }
        if deadline <= env.ledger().timestamp() {
            return Err(CrowdfundError::DeadlinePassed);
        }

        // Validate milestones: count 2-10, sum of pcts = 10000
        Self::validate_milestones(&milestone_descs)?;

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&CrowdfundDataKey::CampaignCount)
            .unwrap_or(0);
        count += 1;
        env.storage()
            .instance()
            .set(&CrowdfundDataKey::CampaignCount, &count);

        // Create escrow pool (0 initial deposit — backers will pledge into it)
        let escrow_addr = Self::get_escrow_addr(&env);
        let pool_args: Vec<Val> = Vec::from_array(
            &env,
            [
                owner.clone().into_val(&env),
                ModuleType::Crowdfund.into_val(&env),
                count.into_val(&env),
                0i128.into_val(&env),
                asset.clone().into_val(&env),
                deadline.into_val(&env),
                env.current_contract_address().into_val(&env),
            ],
        );
        let pool_id: BytesN<32> =
            env.invoke_contract(&escrow_addr, &sym(&env, "create_pool"), pool_args);

        // Store milestones decomposed
        Self::set_milestones(&env, count, &milestone_descs);

        let mut status = CampaignStatus::Draft;
        if submit {
            status = CampaignStatus::Submitted;
        }

        let campaign = Campaign {
            id: count,
            owner: owner.clone(),
            metadata_cid,
            status,
            funding_goal,
            current_funding: 0,
            asset,
            pool_id,
            deadline,
            milestone_count: milestone_descs.len(),
            min_pledge,
            backer_count: 0,
            refund_progress: 0,
            vote_session_id: None,
        };

        let camp_key = CrowdfundDataKey::Campaign(count);
        env.storage().persistent().set(&camp_key, &campaign);
        Self::extend_persistent_ttl(&env, &camp_key);
        Self::extend_instance_ttl(&env);

        CampaignCreated {
            id: count,
            owner,
            funding_goal,
        }
        .publish(&env);

        if submit {
            CampaignSubmittedForReview { id: count }.publish(&env);
        }

        Ok(count)
    }

    pub fn update_campaign(
        env: Env,
        campaign_id: u64,
        metadata_cid: String,
        funding_goal: i128,
        asset: Address,
        deadline: u64,
        milestone_descs: Vec<(String, u32)>,
        min_pledge: i128,
    ) -> Result<(), CrowdfundError> {
        let key = CrowdfundDataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;

        campaign.owner.require_auth();

        if campaign.status != CampaignStatus::Draft {
            return Err(CrowdfundError::NotDraft);
        }

        if funding_goal <= 0 {
            return Err(CrowdfundError::AmountNotPositive);
        }
        if deadline <= env.ledger().timestamp() {
            return Err(CrowdfundError::DeadlinePassed);
        }

        Self::validate_milestones(&milestone_descs)?;
        Self::set_milestones(&env, campaign_id, &milestone_descs);

        campaign.metadata_cid = metadata_cid;
        campaign.funding_goal = funding_goal;
        campaign.asset = asset;
        campaign.deadline = deadline;
        campaign.milestone_count = milestone_descs.len();
        campaign.min_pledge = min_pledge;

        env.storage().persistent().set(&key, &campaign);
        Self::extend_persistent_ttl(&env, &key);

        crate::events::CampaignUpdated {
            id: campaign_id,
            funding_goal,
        }
        .publish(&env);

        Ok(())
    }

    // ========================================================================
    // GOVERNANCE: APPROVAL WORKFLOW
    // Draft → Submitted → Validated → Campaigning
    // ========================================================================

    pub fn submit_for_review(env: Env, campaign_id: u64) -> Result<(), CrowdfundError> {
        let key = CrowdfundDataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;

        campaign.owner.require_auth();

        if campaign.status != CampaignStatus::Draft {
            return Err(CrowdfundError::NotDraft);
        }

        campaign.status = CampaignStatus::Submitted;
        env.storage().persistent().set(&key, &campaign);

        CampaignSubmittedForReview { id: campaign_id }.publish(&env);
        Ok(())
    }

    pub fn approve_campaign(
        env: Env,
        campaign_id: u64,
        voting_duration: u64,
        vote_threshold: u32,
    ) -> Result<BytesN<32>, CrowdfundError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = CrowdfundDataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;

        if campaign.status != CampaignStatus::Submitted {
            return Err(CrowdfundError::NotSubmitted);
        }

        // Create a governance voting session for community validation
        let gov_addr = Self::get_gov_addr(&env);
        let mut options = Vec::new(&env);
        options.push_back(String::from_str(&env, "Approve"));
        options.push_back(String::from_str(&env, "Reject"));

        let now = env.ledger().timestamp();
        let gov_args: Vec<Val> = Vec::from_array(
            &env,
            [
                env.current_contract_address().into_val(&env),
                VoteContext::CampaignValidation.into_val(&env),
                campaign_id.into_val(&env),
                options.into_val(&env),
                now.into_val(&env),
                (now + voting_duration).into_val(&env),
                Some(vote_threshold).into_val(&env),
                None::<u32>.into_val(&env),
                false.into_val(&env),
            ],
        );
        let session_id: BytesN<32> =
            env.invoke_contract(&gov_addr, &sym(&env, "create_session"), gov_args);

        campaign.vote_session_id = Some(session_id.clone());
        campaign.status = CampaignStatus::Submitted; // stays Submitted until votes pass
        env.storage().persistent().set(&key, &campaign);

        CampaignApproved {
            id: campaign_id,
            vote_session_id: session_id.clone(),
        }
        .publish(&env);

        Ok(session_id)
    }

    pub fn reject_campaign(
        env: Env,
        campaign_id: u64,
        reason: String,
    ) -> Result<(), CrowdfundError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = CrowdfundDataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;

        if campaign.status != CampaignStatus::Submitted {
            return Err(CrowdfundError::NotSubmitted);
        }

        campaign.status = CampaignStatus::Draft;
        campaign.vote_session_id = None;
        env.storage().persistent().set(&key, &campaign);

        CampaignRejected {
            id: campaign_id,
            reason,
        }
        .publish(&env);
        Ok(())
    }

    pub fn vote_campaign(
        env: Env,
        voter: Address,
        campaign_id: u64,
        option_id: u32,
    ) -> Result<(), CrowdfundError> {
        voter.require_auth();

        let campaign: Campaign = env
            .storage()
            .persistent()
            .get(&CrowdfundDataKey::Campaign(campaign_id))
            .ok_or(CrowdfundError::CampaignNotFound)?;

        let session_id = campaign
            .vote_session_id
            .ok_or(CrowdfundError::NoVoteSession)?;

        let gov_addr = Self::get_gov_addr(&env);
        let args: Vec<Val> = Vec::from_array(
            &env,
            [
                voter.into_val(&env),
                session_id.into_val(&env),
                option_id.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&gov_addr, &sym(&env, "cast_vote"), args);

        Ok(())
    }

    pub fn check_vote_threshold(env: Env, campaign_id: u64) -> Result<(), CrowdfundError> {
        let key = CrowdfundDataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;

        if campaign.status != CampaignStatus::Submitted {
            return Err(CrowdfundError::NotSubmitted);
        }

        let session_id = campaign
            .vote_session_id
            .clone()
            .ok_or(CrowdfundError::NoVoteSession)?;

        let gov_addr = Self::get_gov_addr(&env);

        // Check if vote threshold has been reached
        let threshold_args: Vec<Val> = Vec::from_array(&env, [session_id.clone().into_val(&env)]);
        let reached: bool =
            env.invoke_contract(&gov_addr, &sym(&env, "threshold_reached"), threshold_args);

        if reached {
            // Threshold reached — check which option won
            let approve_args: Vec<Val> = Vec::from_array(
                &env,
                [
                    session_id.clone().into_val(&env),
                    0u32.into_val(&env), // option 0 = Approve
                ],
            );
            let reject_args: Vec<Val> = Vec::from_array(
                &env,
                [
                    session_id.into_val(&env),
                    1u32.into_val(&env), // option 1 = Reject
                ],
            );

            let approve_option: VoteOption =
                env.invoke_contract(&gov_addr, &sym(&env, "get_option"), approve_args);
            let reject_option: VoteOption =
                env.invoke_contract(&gov_addr, &sym(&env, "get_option"), reject_args);

            let approve_votes = approve_option.votes;
            let reject_votes = reject_option.votes;

            if approve_votes > reject_votes {
                campaign.status = CampaignStatus::Campaigning;
                env.storage().persistent().set(&key, &campaign);
                CampaignValidated { id: campaign_id }.publish(&env);
            } else if reject_votes > approve_votes {
                campaign.status = CampaignStatus::Draft;
                campaign.vote_session_id = None;
                env.storage().persistent().set(&key, &campaign);
                CampaignVoteRejected {
                    id: campaign_id,
                    reason: String::from_str(&env, "reject_majority"),
                }
                .publish(&env);
            } else {
                // Tie — leave state unchanged, session stays open
                return Err(CrowdfundError::VoteThresholdNotMet);
            }
        } else {
            // Threshold not reached — check if voting period has expired
            let session_args: Vec<Val> = Vec::from_array(&env, [session_id.into_val(&env)]);
            let session: VotingSession =
                env.invoke_contract(&gov_addr, &sym(&env, "get_session"), session_args);

            if env.ledger().timestamp() <= session.end_at {
                // Voting still open, threshold not met yet
                return Err(CrowdfundError::VoteThresholdNotMet);
            }

            // Voting expired without reaching threshold — reject
            campaign.status = CampaignStatus::Draft;
            campaign.vote_session_id = None;
            env.storage().persistent().set(&key, &campaign);
            CampaignVoteRejected {
                id: campaign_id,
                reason: String::from_str(&env, "expired_without_approval"),
            }
            .publish(&env);
        }

        Ok(())
    }

    // ========================================================================
    // PLEDGING
    // ========================================================================

    pub fn pledge(
        env: Env,
        backer: Address,
        campaign_id: u64,
        amount: i128,
    ) -> Result<(), CrowdfundError> {
        backer.require_auth();

        let key = CrowdfundDataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;

        if env.ledger().timestamp() > campaign.deadline {
            return Err(CrowdfundError::DeadlinePassed);
        }
        if campaign.status != CampaignStatus::Campaigning {
            return Err(CrowdfundError::NotCampaigning);
        }
        if amount < campaign.min_pledge {
            return Err(CrowdfundError::BelowMinPledge);
        }

        // Use route_pledge for fee-on-top model
        let escrow_addr = Self::get_escrow_addr(&env);
        let pledge_args: Vec<Val> = Vec::from_array(
            &env,
            [
                backer.clone().into_val(&env),
                campaign.pool_id.clone().into_val(&env),
                amount.into_val(&env),
                campaign.asset.clone().into_val(&env),
            ],
        );
        let net: i128 = env.invoke_contract(&escrow_addr, &sym(&env, "route_pledge"), pledge_args);

        campaign.current_funding = campaign
            .current_funding
            .checked_add(net)
            .ok_or(CrowdfundError::Overflow)?;

        // Track backer pledge amount
        let pledge_key = CrowdfundDataKey::Pledge(campaign_id, backer.clone());
        let existing: i128 = env.storage().persistent().get(&pledge_key).unwrap_or(0);
        let new_pledge = existing.checked_add(net).ok_or(CrowdfundError::Overflow)?;
        env.storage().persistent().set(&pledge_key, &new_pledge);

        // Add backer to batch list (if new)
        if existing == 0 {
            let batch_idx = campaign.backer_count / BACKER_BATCH_SIZE;
            let batch_key = CrowdfundDataKey::BackerBatch(campaign_id, batch_idx);
            let mut batch: Vec<Address> = env
                .storage()
                .persistent()
                .get(&batch_key)
                .unwrap_or(Vec::new(&env));
            batch.push_back(backer.clone());
            env.storage().persistent().set(&batch_key, &batch);
            campaign.backer_count += 1;
        }

        // Check if funded
        if campaign.current_funding >= campaign.funding_goal {
            let lock_args: Vec<Val> =
                Vec::from_array(&env, [campaign.pool_id.clone().into_val(&env)]);
            env.invoke_contract::<()>(&escrow_addr, &sym(&env, "lock_pool"), lock_args);

            // Define release slots based on milestones
            let mut slots: Vec<(Address, i128)> = Vec::new(&env);
            for i in 0..campaign.milestone_count {
                let ms: Milestone = env
                    .storage()
                    .persistent()
                    .get(&CrowdfundDataKey::CampaignMilestone(campaign_id, i))
                    .unwrap();
                let ms_amount = campaign
                    .current_funding
                    .checked_mul(ms.pct as i128)
                    .ok_or(CrowdfundError::Overflow)?
                    / 10000;
                slots.push_back((campaign.owner.clone(), ms_amount));
            }
            let slot_args: Vec<Val> = Vec::from_array(
                &env,
                [
                    campaign.pool_id.clone().into_val(&env),
                    slots.into_val(&env),
                ],
            );
            env.invoke_contract::<()>(&escrow_addr, &sym(&env, "define_release_slots"), slot_args);

            campaign.status = CampaignStatus::Funded;
            CampaignFunded { id: campaign_id }.publish(&env);
        }

        env.storage().persistent().set(&key, &campaign);
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);

        PledgeRecorded {
            campaign_id,
            donor: backer,
            amount: net,
        }
        .publish(&env);

        Ok(())
    }

    // ========================================================================
    // MILESTONE MANAGEMENT
    // ========================================================================

    pub fn submit_milestone(
        env: Env,
        campaign_id: u64,
        milestone_index: u32,
    ) -> Result<(), CrowdfundError> {
        let key = CrowdfundDataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;
        campaign.owner.require_auth();

        if campaign.status != CampaignStatus::Funded && campaign.status != CampaignStatus::Executing
        {
            return Err(CrowdfundError::InvalidState);
        }

        let ms_key = CrowdfundDataKey::CampaignMilestone(campaign_id, milestone_index);
        let mut ms: Milestone = env
            .storage()
            .persistent()
            .get(&ms_key)
            .ok_or(CrowdfundError::MilestoneNotFound)?;

        if ms.status != CrowdfundMilestoneStatus::Pending
            && ms.status != CrowdfundMilestoneStatus::Rejected
        {
            return Err(CrowdfundError::MilestoneNotPending);
        }

        ms.status = CrowdfundMilestoneStatus::Submitted;
        env.storage().persistent().set(&ms_key, &ms);

        campaign.status = CampaignStatus::Executing;
        env.storage().persistent().set(&key, &campaign);

        MilestoneSubmitted {
            campaign_id,
            milestone_id: milestone_index,
        }
        .publish(&env);

        Ok(())
    }

    pub fn approve_milestone(
        env: Env,
        campaign_id: u64,
        milestone_index: u32,
    ) -> Result<(), CrowdfundError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = CrowdfundDataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;

        let ms_key = CrowdfundDataKey::CampaignMilestone(campaign_id, milestone_index);
        let mut ms: Milestone = env
            .storage()
            .persistent()
            .get(&ms_key)
            .ok_or(CrowdfundError::MilestoneNotFound)?;

        if ms.status != CrowdfundMilestoneStatus::Submitted {
            return Err(CrowdfundError::MilestoneNotSubmitted);
        }

        ms.status = CrowdfundMilestoneStatus::Released;
        env.storage().persistent().set(&ms_key, &ms);

        // Release the corresponding escrow slot
        let escrow_addr = Self::get_escrow_addr(&env);
        let release_args: Vec<Val> = Vec::from_array(
            &env,
            [
                campaign.pool_id.clone().into_val(&env),
                milestone_index.into_val(&env),
            ],
        );
        env.invoke_contract::<()>(&escrow_addr, &sym(&env, "release_slot"), release_args);

        // Check if all milestones are released
        let mut all_done = true;
        for i in 0..campaign.milestone_count {
            let m: Milestone = env
                .storage()
                .persistent()
                .get(&CrowdfundDataKey::CampaignMilestone(campaign_id, i))
                .unwrap();
            if m.status != CrowdfundMilestoneStatus::Released {
                all_done = false;
                break;
            }
        }

        if all_done {
            campaign.status = CampaignStatus::Completed;

            // Record reputation for campaign owner
            let rep_addr = Self::get_rep_addr(&env);
            let rep_args: Vec<Val> = Vec::from_array(
                &env,
                [
                    env.current_contract_address().into_val(&env),
                    campaign.owner.clone().into_val(&env),
                ],
            );
            env.invoke_contract::<()>(&rep_addr, &sym(&env, "record_campaign_backed"), rep_args);
        }

        env.storage().persistent().set(&key, &campaign);

        MilestoneApproved {
            campaign_id,
            milestone_id: milestone_index,
        }
        .publish(&env);

        Ok(())
    }

    pub fn reject_milestone(
        env: Env,
        campaign_id: u64,
        milestone_index: u32,
    ) -> Result<(), CrowdfundError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let ms_key = CrowdfundDataKey::CampaignMilestone(campaign_id, milestone_index);
        let mut ms: Milestone = env
            .storage()
            .persistent()
            .get(&ms_key)
            .ok_or(CrowdfundError::MilestoneNotFound)?;

        if ms.status != CrowdfundMilestoneStatus::Submitted {
            return Err(CrowdfundError::MilestoneNotSubmitted);
        }

        ms.status = CrowdfundMilestoneStatus::Rejected;
        env.storage().persistent().set(&ms_key, &ms);

        Ok(())
    }

    pub fn request_milestone_revision(
        env: Env,
        campaign_id: u64,
        milestone_index: u32,
    ) -> Result<(), CrowdfundError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let ms_key = CrowdfundDataKey::CampaignMilestone(campaign_id, milestone_index);
        let mut ms: Milestone = env
            .storage()
            .persistent()
            .get(&ms_key)
            .ok_or(CrowdfundError::MilestoneNotFound)?;

        if ms.status != CrowdfundMilestoneStatus::Submitted
            && ms.status != CrowdfundMilestoneStatus::Disputed
        {
            return Err(CrowdfundError::MilestoneNotSubmitted);
        }

        ms.status = CrowdfundMilestoneStatus::Pending;
        env.storage().persistent().set(&ms_key, &ms);

        MilestoneRevisionRequested {
            campaign_id,
            milestone_id: milestone_index,
        }
        .publish(&env);

        Ok(())
    }

    // ========================================================================
    // DEADLINE / FAILURE
    // ========================================================================

    pub fn check_deadline(env: Env, campaign_id: u64) -> Result<(), CrowdfundError> {
        let key = CrowdfundDataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;

        if env.ledger().timestamp() <= campaign.deadline {
            return Err(CrowdfundError::DeadlineNotPassed);
        }
        if campaign.status != CampaignStatus::Campaigning {
            return Err(CrowdfundError::InvalidState);
        }

        campaign.status = CampaignStatus::Failed;
        campaign.refund_progress = 0;
        env.storage().persistent().set(&key, &campaign);

        CampaignFailed { id: campaign_id }.publish(&env);
        Ok(())
    }

    pub fn process_refund_batch(env: Env, campaign_id: u64) -> Result<(), CrowdfundError> {
        let key = CrowdfundDataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;

        if campaign.status != CampaignStatus::Failed && campaign.status != CampaignStatus::Cancelled
        {
            return Err(CrowdfundError::InvalidState);
        }

        let batch_idx = campaign.refund_progress;
        let total_batches = campaign.backer_count.div_ceil(BACKER_BATCH_SIZE);
        if batch_idx >= total_batches {
            return Err(CrowdfundError::RefundBatchDone);
        }

        let batch_key = CrowdfundDataKey::BackerBatch(campaign_id, batch_idx);
        let batch: Vec<Address> = env
            .storage()
            .persistent()
            .get(&batch_key)
            .unwrap_or(Vec::new(&env));

        // Build refund list
        let mut refund_list: Vec<(Address, i128)> = Vec::new(&env);
        let mut count = 0u32;

        for backer in batch.iter() {
            let pledge_key = CrowdfundDataKey::Pledge(campaign_id, backer.clone());
            let amount: i128 = env.storage().persistent().get(&pledge_key).unwrap_or(0);
            if amount > 0 {
                refund_list.push_back((backer.clone(), amount));
                env.storage().persistent().set(&pledge_key, &0i128);
                count += 1;
            }
        }

        if !refund_list.is_empty() {
            let escrow_addr = Self::get_escrow_addr(&env);
            let refund_args: Vec<Val> = Vec::from_array(
                &env,
                [
                    campaign.pool_id.clone().into_val(&env),
                    refund_list.into_val(&env),
                ],
            );
            env.invoke_contract::<()>(&escrow_addr, &sym(&env, "refund_backers"), refund_args);
        }

        campaign.refund_progress = batch_idx + 1;
        env.storage().persistent().set(&key, &campaign);

        RefundBatchProcessed {
            campaign_id,
            batch_index: batch_idx,
            count,
        }
        .publish(&env);

        Ok(())
    }

    // ========================================================================
    // CANCELLATION (admin)
    // ========================================================================

    pub fn cancel_campaign(env: Env, campaign_id: u64) -> Result<(), CrowdfundError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = CrowdfundDataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;

        if campaign.status == CampaignStatus::Completed {
            return Err(CrowdfundError::InvalidState);
        }

        campaign.status = CampaignStatus::Cancelled;
        campaign.refund_progress = 0;
        env.storage().persistent().set(&key, &campaign);

        CampaignCancelled { id: campaign_id }.publish(&env);
        Ok(())
    }

    // ========================================================================
    // GOVERNANCE: DISPUTE & TERMINATION
    // ========================================================================

    pub fn dispute_milestone(
        env: Env,
        disputer: Address,
        campaign_id: u64,
        milestone_index: u32,
    ) -> Result<(), CrowdfundError> {
        disputer.require_auth();

        // Verify disputer is a backer
        let pledge_key = CrowdfundDataKey::Pledge(campaign_id, disputer.clone());
        let pledge_amount: i128 = env.storage().persistent().get(&pledge_key).unwrap_or(0);
        if pledge_amount <= 0 {
            return Err(CrowdfundError::NotBacker);
        }

        let ms_key = CrowdfundDataKey::CampaignMilestone(campaign_id, milestone_index);
        let mut ms: Milestone = env
            .storage()
            .persistent()
            .get(&ms_key)
            .ok_or(CrowdfundError::MilestoneNotFound)?;

        if ms.status != CrowdfundMilestoneStatus::Submitted {
            return Err(CrowdfundError::MilestoneNotSubmitted);
        }

        ms.status = CrowdfundMilestoneStatus::Disputed;
        env.storage().persistent().set(&ms_key, &ms);

        MilestoneDisputed {
            campaign_id,
            milestone_id: milestone_index,
            disputer,
        }
        .publish(&env);

        Ok(())
    }

    pub fn resolve_dispute(
        env: Env,
        campaign_id: u64,
        milestone_index: u32,
        resolution: DisputeResolution,
    ) -> Result<(), CrowdfundError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = CrowdfundDataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;

        let ms_key = CrowdfundDataKey::CampaignMilestone(campaign_id, milestone_index);
        let mut ms: Milestone = env
            .storage()
            .persistent()
            .get(&ms_key)
            .ok_or(CrowdfundError::MilestoneNotFound)?;

        if ms.status != CrowdfundMilestoneStatus::Disputed {
            return Err(CrowdfundError::MilestoneNotDisputed);
        }

        match resolution {
            DisputeResolution::ApproveCreator => {
                // Resolve in favor of creator: release milestone funds
                ms.status = CrowdfundMilestoneStatus::Released;
                env.storage().persistent().set(&ms_key, &ms);

                let escrow_addr = Self::get_escrow_addr(&env);
                let release_args: Vec<Val> = Vec::from_array(
                    &env,
                    [
                        campaign.pool_id.clone().into_val(&env),
                        milestone_index.into_val(&env),
                    ],
                );
                env.invoke_contract::<()>(&escrow_addr, &sym(&env, "release_slot"), release_args);

                // Check if all milestones are released
                let mut all_done = true;
                for i in 0..campaign.milestone_count {
                    let m: Milestone = env
                        .storage()
                        .persistent()
                        .get(&CrowdfundDataKey::CampaignMilestone(campaign_id, i))
                        .unwrap();
                    if m.status != CrowdfundMilestoneStatus::Released {
                        all_done = false;
                        break;
                    }
                }

                if all_done {
                    campaign.status = CampaignStatus::Completed;

                    let rep_addr = Self::get_rep_addr(&env);
                    let rep_args: Vec<Val> = Vec::from_array(
                        &env,
                        [
                            env.current_contract_address().into_val(&env),
                            campaign.owner.clone().into_val(&env),
                        ],
                    );
                    env.invoke_contract::<()>(
                        &rep_addr,
                        &sym(&env, "record_campaign_backed"),
                        rep_args,
                    );
                }

                env.storage().persistent().set(&key, &campaign);
            }
            DisputeResolution::ApproveBacker => {
                // Resolve in favor of backer: reject milestone, cancel campaign for refunds
                ms.status = CrowdfundMilestoneStatus::Rejected;
                env.storage().persistent().set(&ms_key, &ms);

                campaign.status = CampaignStatus::Cancelled;
                campaign.refund_progress = 0;
                env.storage().persistent().set(&key, &campaign);
            }
        }

        DisputeResolved {
            campaign_id,
            milestone_id: milestone_index,
            resolution,
        }
        .publish(&env);

        Self::extend_instance_ttl(&env);
        Ok(())
    }

    pub fn terminate_campaign(env: Env, campaign_id: u64) -> Result<(), CrowdfundError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = CrowdfundDataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;

        if campaign.status == CampaignStatus::Completed
            || campaign.status == CampaignStatus::Cancelled
            || campaign.status == CampaignStatus::Failed
        {
            return Err(CrowdfundError::InvalidState);
        }

        campaign.status = CampaignStatus::Cancelled;
        campaign.refund_progress = 0;
        env.storage().persistent().set(&key, &campaign);

        CampaignTerminated { id: campaign_id }.publish(&env);
        Ok(())
    }

    pub fn flag_overdue_milestone(
        env: Env,
        campaign_id: u64,
        milestone_index: u32,
    ) -> Result<(), CrowdfundError> {
        let key = CrowdfundDataKey::Campaign(campaign_id);
        let campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(CrowdfundError::CampaignNotFound)?;

        if campaign.status != CampaignStatus::Funded && campaign.status != CampaignStatus::Executing
        {
            return Err(CrowdfundError::InvalidState);
        }

        let ms_key = CrowdfundDataKey::CampaignMilestone(campaign_id, milestone_index);
        let ms: Milestone = env
            .storage()
            .persistent()
            .get(&ms_key)
            .ok_or(CrowdfundError::MilestoneNotFound)?;

        if ms.status != CrowdfundMilestoneStatus::Pending {
            return Err(CrowdfundError::MilestoneNotPending);
        }

        // Check if 30 days have passed since deadline (which marks funding time)
        let overdue_threshold = campaign.deadline + 30 * 86_400;
        if env.ledger().timestamp() <= overdue_threshold {
            return Err(CrowdfundError::MilestoneNotOverdue);
        }

        MilestoneOverdue {
            campaign_id,
            milestone_id: milestone_index,
        }
        .publish(&env);

        Ok(())
    }

    // ========================================================================
    // ADMIN
    // ========================================================================

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), CrowdfundError> {
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

    fn extend_persistent_ttl(env: &Env, key: &CrowdfundDataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND);
    }

    fn require_admin(env: &Env) -> Result<Address, CrowdfundError> {
        env.storage()
            .instance()
            .get(&CrowdfundDataKey::Admin)
            .ok_or(CrowdfundError::NotInitialized)
    }

    fn get_escrow_addr(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&CrowdfundDataKey::CoreEscrow)
            .expect("not initialized")
    }

    fn get_rep_addr(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&CrowdfundDataKey::ReputationRegistry)
            .expect("not initialized")
    }

    fn get_gov_addr(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&CrowdfundDataKey::GovernanceVoting)
            .expect("not initialized")
    }

    fn validate_milestones(milestone_descs: &Vec<(String, u32)>) -> Result<(), CrowdfundError> {
        let ms_count = milestone_descs.len();
        if !(2..=10).contains(&ms_count) {
            return Err(CrowdfundError::InvalidMilestones);
        }
        let mut pct_sum: u32 = 0;
        for i in 0..ms_count {
            let (_, pct) = milestone_descs.get(i).unwrap();
            pct_sum += pct;
        }
        if pct_sum != 10000 {
            return Err(CrowdfundError::InvalidMilestones);
        }
        Ok(())
    }

    fn set_milestones(env: &Env, campaign_id: u64, milestone_descs: &Vec<(String, u32)>) {
        for i in 0..milestone_descs.len() {
            let (desc, pct) = milestone_descs.get(i).unwrap();
            let milestone = Milestone {
                id: i,
                description: desc,
                pct,
                status: CrowdfundMilestoneStatus::Pending,
            };
            env.storage().persistent().set(
                &CrowdfundDataKey::CampaignMilestone(campaign_id, i),
                &milestone,
            );
        }
    }
}
