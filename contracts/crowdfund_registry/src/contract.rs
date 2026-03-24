use crate::error::Error;
use crate::events::{
    CampaignApproved, CampaignCancelled, CampaignCreated, CampaignFailed, CampaignFunded,
    CampaignRejected, CampaignSubmittedForReview, CampaignTerminated, CampaignValidated,
    MilestoneApproved, MilestoneDisputed, MilestoneOverdue, MilestoneSubmitted, PledgeRecorded,
    RefundBatchProcessed,
};
use crate::storage::{Campaign, CampaignStatus, DataKey, Milestone, MilestoneStatus};
use boundless_types::ttl::{
    INSTANCE_TTL_EXTEND, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD,
};
use boundless_types::ModuleType;
use core_escrow::CoreEscrowClient;
use governance_voting::storage::VoteContext;
use governance_voting::GovernanceVotingClient;
use reputation_registry::ReputationRegistryClient;
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

const BACKER_BATCH_SIZE: u32 = 50;

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
        env.storage().instance().set(&DataKey::CampaignCount, &0u64);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    // ========================================================================
    // QUERIES
    // ========================================================================

    pub fn get_campaign(env: Env, campaign_id: u64) -> Result<Campaign, Error> {
        let key = DataKey::Campaign(campaign_id);
        let campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::CampaignNotFound)?;
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);
        Ok(campaign)
    }

    pub fn get_milestone(
        env: Env,
        campaign_id: u64,
        milestone_index: u32,
    ) -> Result<Milestone, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::CampaignMilestone(campaign_id, milestone_index))
            .ok_or(Error::MilestoneNotFound)
    }

    pub fn get_pledge(env: Env, campaign_id: u64, backer: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Pledge(campaign_id, backer))
            .unwrap_or(0)
    }

    pub fn get_campaign_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::CampaignCount)
            .unwrap_or(0)
    }

    pub fn get_vote_session(env: Env, campaign_id: u64) -> Result<BytesN<32>, Error> {
        let campaign: Campaign = env
            .storage()
            .persistent()
            .get(&DataKey::Campaign(campaign_id))
            .ok_or(Error::CampaignNotFound)?;
        campaign.vote_session_id.ok_or(Error::NoVoteSession)
    }

    // ========================================================================
    // CAMPAIGN CREATION (starts in Draft)
    // ========================================================================

    /// Create a campaign with decomposed milestones. Starts in Draft status.
    /// Owner must call `submit_for_review` to advance to admin approval workflow.
    pub fn create_campaign(
        env: Env,
        owner: Address,
        metadata_cid: String,
        funding_goal: i128,
        asset: Address,
        deadline: u64,
        milestone_descs: Vec<(String, u32)>,
        min_pledge: i128,
    ) -> Result<u64, Error> {
        owner.require_auth();

        if funding_goal <= 0 {
            return Err(Error::AmountNotPositive);
        }
        if deadline <= env.ledger().timestamp() {
            return Err(Error::DeadlinePassed);
        }

        // Validate milestones: count 2-10, sum of pcts = 10000
        let ms_count = milestone_descs.len();
        if ms_count < 2 || ms_count > 10 {
            return Err(Error::InvalidMilestones);
        }
        let mut pct_sum: u32 = 0;
        for i in 0..ms_count {
            let (_, pct) = milestone_descs.get(i).unwrap();
            pct_sum += pct;
        }
        if pct_sum != 10000 {
            return Err(Error::InvalidMilestones);
        }

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::CampaignCount)
            .unwrap_or(0);
        count += 1;
        env.storage().instance().set(&DataKey::CampaignCount, &count);

        // Create escrow pool (0 initial deposit — backers will pledge into it)
        let escrow_client = Self::escrow_client(&env);
        let pool_id = escrow_client.create_pool(
            &owner,
            &ModuleType::Crowdfund,
            &count,
            &0i128,
            &asset,
            &deadline,
            &env.current_contract_address(),
        );

        // Store milestones decomposed
        for i in 0..ms_count {
            let (desc, pct) = milestone_descs.get(i).unwrap();
            let milestone = Milestone {
                id: i,
                description: desc,
                pct,
                status: MilestoneStatus::Pending,
            };
            env.storage()
                .persistent()
                .set(&DataKey::CampaignMilestone(count, i), &milestone);
        }

        let campaign = Campaign {
            id: count,
            owner: owner.clone(),
            metadata_cid,
            status: CampaignStatus::Draft,
            funding_goal,
            current_funding: 0,
            asset,
            pool_id,
            deadline,
            milestone_count: ms_count,
            min_pledge,
            backer_count: 0,
            refund_progress: 0,
            vote_session_id: None,
        };

        let camp_key = DataKey::Campaign(count);
        env.storage()
            .persistent()
            .set(&camp_key, &campaign);
        Self::extend_persistent_ttl(&env, &camp_key);
        Self::extend_instance_ttl(&env);

        CampaignCreated {
            id: count,
            owner,
            funding_goal,
        }
        .publish(&env);

        Ok(count)
    }

    // ========================================================================
    // GOVERNANCE: APPROVAL WORKFLOW
    // Draft → Submitted → Validated → Campaigning
    // ========================================================================

    /// Owner submits their draft campaign for admin review.
    pub fn submit_for_review(env: Env, campaign_id: u64) -> Result<(), Error> {
        let key = DataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::CampaignNotFound)?;

        campaign.owner.require_auth();

        if campaign.status != CampaignStatus::Draft {
            return Err(Error::NotDraft);
        }

        campaign.status = CampaignStatus::Submitted;
        env.storage().persistent().set(&key, &campaign);

        CampaignSubmittedForReview { id: campaign_id }.publish(&env);
        Ok(())
    }

    /// Admin approves a submitted campaign. Creates a GovernanceVoting session
    /// for community validation. Campaign moves to Validated once votes pass threshold.
    pub fn approve_campaign(
        env: Env,
        campaign_id: u64,
        voting_duration: u64,
        vote_threshold: u32,
    ) -> Result<BytesN<32>, Error> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = DataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::CampaignNotFound)?;

        if campaign.status != CampaignStatus::Submitted {
            return Err(Error::NotSubmitted);
        }

        // Create a governance voting session for community validation
        let gov_client = Self::gov_client(&env);

        let mut options = Vec::new(&env);
        options.push_back(String::from_str(&env, "Approve"));
        options.push_back(String::from_str(&env, "Reject"));

        let now = env.ledger().timestamp();
        let session_id = gov_client.create_session(
            &env.current_contract_address(),
            &VoteContext::CampaignValidation,
            &campaign_id,
            &options,
            &now,
            &(now + voting_duration),
            &Some(vote_threshold),
            &None::<u32>,
            &false,
        );

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

    /// Admin rejects a submitted campaign. Returns to Draft so owner can revise.
    pub fn reject_campaign(env: Env, campaign_id: u64) -> Result<(), Error> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = DataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::CampaignNotFound)?;

        if campaign.status != CampaignStatus::Submitted {
            return Err(Error::NotSubmitted);
        }

        campaign.status = CampaignStatus::Draft;
        campaign.vote_session_id = None;
        env.storage().persistent().set(&key, &campaign);

        CampaignRejected { id: campaign_id }.publish(&env);
        Ok(())
    }

    /// Delegates vote to GovernanceVoting. Anyone can vote on an approved campaign's session.
    pub fn vote_campaign(
        env: Env,
        voter: Address,
        campaign_id: u64,
        option_id: u32,
    ) -> Result<(), Error> {
        voter.require_auth();

        let campaign: Campaign = env
            .storage()
            .persistent()
            .get(&DataKey::Campaign(campaign_id))
            .ok_or(Error::CampaignNotFound)?;

        let session_id = campaign.vote_session_id.ok_or(Error::NoVoteSession)?;

        let gov_client = Self::gov_client(&env);
        gov_client.cast_vote(&voter, &session_id, &option_id);

        Ok(())
    }

    /// Permissionless: checks if the voting threshold has been reached.
    /// If yes, transitions campaign from Submitted → Campaigning.
    pub fn check_vote_threshold(env: Env, campaign_id: u64) -> Result<(), Error> {
        let key = DataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::CampaignNotFound)?;

        if campaign.status != CampaignStatus::Submitted {
            return Err(Error::NotSubmitted);
        }

        let session_id = campaign.vote_session_id.clone().ok_or(Error::NoVoteSession)?;

        let gov_client = Self::gov_client(&env);
        let reached = gov_client.threshold_reached(&session_id);

        if !reached {
            return Err(Error::VoteThresholdNotMet);
        }

        campaign.status = CampaignStatus::Campaigning;
        env.storage().persistent().set(&key, &campaign);

        CampaignValidated { id: campaign_id }.publish(&env);
        Ok(())
    }

    // ========================================================================
    // PLEDGING
    // ========================================================================

    pub fn pledge(env: Env, backer: Address, campaign_id: u64, amount: i128) -> Result<(), Error> {
        backer.require_auth();

        let key = DataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
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

        // Use route_pledge for fee-on-top model
        let escrow_client = Self::escrow_client(&env);
        let net = escrow_client.route_pledge(
            &backer,
            &campaign.pool_id,
            &amount,
            &campaign.asset,
        );

        campaign.current_funding = campaign.current_funding.checked_add(net).ok_or(Error::Overflow)?;

        // Track backer pledge amount
        let pledge_key = DataKey::Pledge(campaign_id, backer.clone());
        let existing: i128 = env.storage().persistent().get(&pledge_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&pledge_key, &(existing + net));

        // Add backer to batch list (if new)
        if existing == 0 {
            let batch_idx = campaign.backer_count / BACKER_BATCH_SIZE;
            let batch_key = DataKey::BackerBatch(campaign_id, batch_idx);
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
            escrow_client.lock_pool(&campaign.pool_id);

            // Define release slots based on milestones
            let mut slots = Vec::new(&env);
            for i in 0..campaign.milestone_count {
                let ms: Milestone = env
                    .storage()
                    .persistent()
                    .get(&DataKey::CampaignMilestone(campaign_id, i))
                    .unwrap();
                let ms_amount =
                    (campaign.current_funding * ms.pct as i128) / 10000;
                slots.push_back((campaign.owner.clone(), ms_amount));
            }
            escrow_client.define_release_slots(&campaign.pool_id, &slots);

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
    ) -> Result<(), Error> {
        let key = DataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::CampaignNotFound)?;
        campaign.owner.require_auth();

        if campaign.status != CampaignStatus::Funded
            && campaign.status != CampaignStatus::Executing
        {
            return Err(Error::InvalidState);
        }

        let ms_key = DataKey::CampaignMilestone(campaign_id, milestone_index);
        let mut ms: Milestone = env
            .storage()
            .persistent()
            .get(&ms_key)
            .ok_or(Error::MilestoneNotFound)?;

        if ms.status != MilestoneStatus::Pending && ms.status != MilestoneStatus::Rejected {
            return Err(Error::MilestoneNotPending);
        }

        ms.status = MilestoneStatus::Submitted;
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
    ) -> Result<(), Error> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = DataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::CampaignNotFound)?;

        let ms_key = DataKey::CampaignMilestone(campaign_id, milestone_index);
        let mut ms: Milestone = env
            .storage()
            .persistent()
            .get(&ms_key)
            .ok_or(Error::MilestoneNotFound)?;

        if ms.status != MilestoneStatus::Submitted {
            return Err(Error::MilestoneNotSubmitted);
        }

        ms.status = MilestoneStatus::Released;
        env.storage().persistent().set(&ms_key, &ms);

        // Release the corresponding escrow slot
        let escrow_client = Self::escrow_client(&env);
        escrow_client.release_slot(&campaign.pool_id, &milestone_index);

        // Check if all milestones are released
        let mut all_done = true;
        for i in 0..campaign.milestone_count {
            let m: Milestone = env
                .storage()
                .persistent()
                .get(&DataKey::CampaignMilestone(campaign_id, i))
                .unwrap();
            if m.status != MilestoneStatus::Released {
                all_done = false;
                break;
            }
        }

        if all_done {
            campaign.status = CampaignStatus::Completed;

            // Record reputation for campaign owner
            let rep_client = Self::rep_client(&env);
            rep_client.record_campaign_backed(
                &env.current_contract_address(),
                &campaign.owner,
            );
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
    ) -> Result<(), Error> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let ms_key = DataKey::CampaignMilestone(campaign_id, milestone_index);
        let mut ms: Milestone = env
            .storage()
            .persistent()
            .get(&ms_key)
            .ok_or(Error::MilestoneNotFound)?;

        if ms.status != MilestoneStatus::Submitted {
            return Err(Error::MilestoneNotSubmitted);
        }

        ms.status = MilestoneStatus::Rejected;
        env.storage().persistent().set(&ms_key, &ms);

        Ok(())
    }

    // ========================================================================
    // DEADLINE / FAILURE
    // ========================================================================

    /// Permissionless: anyone can call after deadline if not funded.
    pub fn check_deadline(env: Env, campaign_id: u64) -> Result<(), Error> {
        let key = DataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::CampaignNotFound)?;

        if env.ledger().timestamp() <= campaign.deadline {
            return Err(Error::DeadlineNotPassed);
        }
        if campaign.status != CampaignStatus::Campaigning {
            return Err(Error::InvalidState);
        }

        campaign.status = CampaignStatus::Failed;
        campaign.refund_progress = 0;
        env.storage().persistent().set(&key, &campaign);

        CampaignFailed { id: campaign_id }.publish(&env);
        Ok(())
    }

    /// Permissionless batched refund: processes one batch of backers at a time.
    pub fn process_refund_batch(env: Env, campaign_id: u64) -> Result<(), Error> {
        let key = DataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::CampaignNotFound)?;

        if campaign.status != CampaignStatus::Failed
            && campaign.status != CampaignStatus::Cancelled
        {
            return Err(Error::InvalidState);
        }

        let batch_idx = campaign.refund_progress;
        let total_batches = (campaign.backer_count + BACKER_BATCH_SIZE - 1) / BACKER_BATCH_SIZE;
        if batch_idx >= total_batches {
            return Err(Error::RefundBatchDone);
        }

        let batch_key = DataKey::BackerBatch(campaign_id, batch_idx);
        let batch: Vec<Address> = env
            .storage()
            .persistent()
            .get(&batch_key)
            .unwrap_or(Vec::new(&env));

        // Build refund list
        let escrow_client = Self::escrow_client(&env);
        let mut refund_list = Vec::new(&env);
        let mut count = 0u32;

        for backer in batch.iter() {
            let pledge_key = DataKey::Pledge(campaign_id, backer.clone());
            let amount: i128 = env.storage().persistent().get(&pledge_key).unwrap_or(0);
            if amount > 0 {
                refund_list.push_back((backer.clone(), amount));
                env.storage().persistent().set(&pledge_key, &0i128);
                count += 1;
            }
        }

        if !refund_list.is_empty() {
            escrow_client.refund_backers(&campaign.pool_id, &refund_list);
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

    pub fn cancel_campaign(env: Env, campaign_id: u64) -> Result<(), Error> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = DataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::CampaignNotFound)?;

        if campaign.status == CampaignStatus::Completed {
            return Err(Error::InvalidState);
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

    /// A backer disputes a submitted milestone.
    pub fn dispute_milestone(
        env: Env,
        disputer: Address,
        campaign_id: u64,
        milestone_index: u32,
    ) -> Result<(), Error> {
        disputer.require_auth();

        // Verify disputer is a backer
        let pledge_key = DataKey::Pledge(campaign_id, disputer.clone());
        let pledge_amount: i128 = env.storage().persistent().get(&pledge_key).unwrap_or(0);
        if pledge_amount <= 0 {
            return Err(Error::NotBacker);
        }

        let ms_key = DataKey::CampaignMilestone(campaign_id, milestone_index);
        let mut ms: Milestone = env
            .storage()
            .persistent()
            .get(&ms_key)
            .ok_or(Error::MilestoneNotFound)?;

        if ms.status != MilestoneStatus::Submitted {
            return Err(Error::MilestoneNotSubmitted);
        }

        ms.status = MilestoneStatus::Disputed;
        env.storage().persistent().set(&ms_key, &ms);

        MilestoneDisputed {
            campaign_id,
            milestone_id: milestone_index,
            disputer,
        }
        .publish(&env);

        Ok(())
    }

    /// Admin terminates a campaign (e.g., due to fraud). Triggers refund flow.
    pub fn terminate_campaign(env: Env, campaign_id: u64) -> Result<(), Error> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = DataKey::Campaign(campaign_id);
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::CampaignNotFound)?;

        if campaign.status == CampaignStatus::Completed
            || campaign.status == CampaignStatus::Cancelled
            || campaign.status == CampaignStatus::Failed
        {
            return Err(Error::InvalidState);
        }

        campaign.status = CampaignStatus::Cancelled;
        campaign.refund_progress = 0;
        env.storage().persistent().set(&key, &campaign);

        CampaignTerminated { id: campaign_id }.publish(&env);
        Ok(())
    }

    /// Permissionless: flag a milestone as overdue if campaign is Executing
    /// and the milestone has been pending for too long (>30 days past funding).
    pub fn flag_overdue_milestone(
        env: Env,
        campaign_id: u64,
        milestone_index: u32,
    ) -> Result<(), Error> {
        let key = DataKey::Campaign(campaign_id);
        let campaign: Campaign = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::CampaignNotFound)?;

        if campaign.status != CampaignStatus::Funded
            && campaign.status != CampaignStatus::Executing
        {
            return Err(Error::InvalidState);
        }

        let ms_key = DataKey::CampaignMilestone(campaign_id, milestone_index);
        let ms: Milestone = env
            .storage()
            .persistent()
            .get(&ms_key)
            .ok_or(Error::MilestoneNotFound)?;

        if ms.status != MilestoneStatus::Pending {
            return Err(Error::MilestoneNotPending);
        }

        // Check if 30 days have passed since deadline (which marks funding time)
        let overdue_threshold = campaign.deadline + 30 * 86_400;
        if env.ledger().timestamp() <= overdue_threshold {
            return Err(Error::MilestoneNotOverdue);
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

    fn require_admin(env: &Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
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
}
