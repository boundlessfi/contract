use crate::contract::{CrowdfundRegistry, CrowdfundRegistryClient};
use crate::storage::{CampaignStatus, CrowdfundMilestoneStatus, DisputeResolution};
use core_escrow::{CoreEscrow, CoreEscrowClient};
use governance_voting::{GovernanceVoting, GovernanceVotingClient};
use reputation_registry::{ReputationRegistry, ReputationRegistryClient};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, Env, Vec};

#[allow(dead_code)]
struct TestEnv<'a> {
    env: Env,
    client: CrowdfundRegistryClient<'a>,
    escrow_client: CoreEscrowClient<'a>,
    rep_client: ReputationRegistryClient<'a>,
    gov_client: GovernanceVotingClient<'a>,
    admin: Address,
    token: TokenClient<'a>,
    token_addr: Address,
}

fn setup() -> TestEnv<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    // Deploy token
    let token_admin = Address::generate(&env);
    let token_addr = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token = TokenClient::new(&env, &token_addr);
    let sac = StellarAssetClient::new(&env, &token_addr);

    // Deploy CoreEscrow
    let escrow_id = env.register(CoreEscrow, ());
    let escrow_client = CoreEscrowClient::new(&env, &escrow_id);
    escrow_client.init(&admin, &treasury);

    // Deploy ReputationRegistry
    let rep_id = env.register(ReputationRegistry, ());
    let rep_client = ReputationRegistryClient::new(&env, &rep_id);
    rep_client.init(&admin);

    // Deploy GovernanceVoting
    let gov_id = env.register(GovernanceVoting, ());
    let gov_client = GovernanceVotingClient::new(&env, &gov_id);
    gov_client.init(&admin);

    // Deploy CrowdfundRegistry
    let cf_id = env.register(CrowdfundRegistry, ());
    let client = CrowdfundRegistryClient::new(&env, &cf_id);
    client.init(&admin, &escrow_id, &rep_id, &gov_id);

    // Authorize CrowdfundRegistry in CoreEscrow, ReputationRegistry, GovernanceVoting
    escrow_client.authorize_module(&cf_id);
    rep_client.add_authorized_module(&cf_id);
    gov_client.add_authorized_module(&cf_id);

    // Mint tokens to donors
    sac.mint(&admin, &100_000);

    TestEnv {
        env,
        client,
        escrow_client,
        rep_client,
        gov_client,
        admin,
        token,
        token_addr,
    }
}

/// Two equal-weight milestones (50% / 50%).
fn make_milestones(env: &Env) -> Vec<u32> {
    let mut ms = Vec::new(env);
    ms.push_back(5000u32);
    ms.push_back(5000u32);
    ms
}

/// Advance a campaign from Submitted → Approved (vote session) → Campaigning.
fn advance_to_campaigning(t: &TestEnv, campaign_id: u64) {
    // Admin approves (duration=1000 ledger seconds, threshold=1 vote)
    let _session_id = t.client.approve_campaign(&campaign_id, &1000, &1);
    // A single voter approves (option 0 = "Approve")
    let voter = Address::generate(&t.env);
    t.client.vote_campaign(&voter, &campaign_id, &0);
    t.client.check_vote_threshold(&campaign_id);
}

#[test]
fn test_create_campaign() {
    let t = setup();
    let owner = t.admin.clone();

    let cid = t.client.create_campaign(
        &owner,
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    assert_eq!(cid, 1);
    let campaign = t.client.get_campaign(&1);
    assert_eq!(campaign.status, CampaignStatus::Submitted);
    assert_eq!(campaign.funding_goal, 10000);
    assert_eq!(campaign.milestone_count, 2);
}

#[test]
fn test_governance_flow() {
    let t = setup();
    let owner = t.admin.clone();

    let cid = t.client.create_campaign(
        &owner,
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    assert_eq!(
        t.client.get_campaign(&cid).status,
        CampaignStatus::Submitted
    );

    // Admin approves → creates vote session
    let session_id = t.client.approve_campaign(&cid, &1000, &1);
    assert_eq!(
        t.client.get_campaign(&cid).status,
        CampaignStatus::Submitted
    );
    assert_eq!(t.client.get_vote_session(&cid), session_id);

    // Vote
    let voter = Address::generate(&t.env);
    t.client.vote_campaign(&voter, &cid, &0);

    // Check threshold → Campaigning
    t.client.check_vote_threshold(&cid);
    assert_eq!(
        t.client.get_campaign(&cid).status,
        CampaignStatus::Campaigning
    );
}

#[test]
fn test_reject_campaign() {
    let t = setup();
    let owner = t.admin.clone();

    let cid = t.client.create_campaign(
        &owner,
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    // Rejection reason is stored in the backend DB, not on-chain.
    t.client.reject_campaign(&cid);
    assert_eq!(
        t.client.get_campaign(&cid).status,
        CampaignStatus::Cancelled
    );
}

#[test]
fn test_create_campaign_starts_submitted() {
    let t = setup();
    let owner = t.admin.clone();

    let cid = t.client.create_campaign(
        &owner,
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    // Campaigns start directly as Submitted — no draft step.
    assert_eq!(
        t.client.get_campaign(&cid).status,
        CampaignStatus::Submitted
    );
}

#[test]
fn test_update_campaign() {
    let t = setup();
    let owner = t.admin.clone();

    let cid = t.client.create_campaign(
        &owner,
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    let new_goal = 20000i128;
    let mut new_ms = Vec::new(&t.env);
    new_ms.push_back(5000u32);
    new_ms.push_back(5000u32);

    t.client.update_campaign(
        &cid,
        &new_goal,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 90000),
        &new_ms,
        &200i128,
    );

    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.funding_goal, new_goal);
    assert_eq!(campaign.milestone_count, 2);
    assert_eq!(campaign.min_pledge, 200);
}

#[test]
fn test_full_lifecycle() {
    let t = setup();
    let sac = StellarAssetClient::new(&t.env, &t.token_addr);

    let owner = Address::generate(&t.env);
    let donor1 = Address::generate(&t.env);
    let donor2 = Address::generate(&t.env);

    sac.mint(&donor1, &10_000);
    sac.mint(&donor2, &10_000);

    let cid = t.client.create_campaign(
        &owner,
        &1000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    advance_to_campaigning(&t, cid);

    // Pledge enough to fund
    t.client.pledge(&donor1, &cid, &600);
    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Campaigning);

    t.client.pledge(&donor2, &cid, &500);
    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Funded);

    // Submit and approve milestone 0
    t.client.submit_milestone(&cid, &0);
    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Executing);

    t.client.approve_milestone(&cid, &0);
    assert!(t.token.balance(&owner) > 0);

    // Submit and approve milestone 1 → Completed
    t.client.submit_milestone(&cid, &1);
    t.client.approve_milestone(&cid, &1);

    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Completed);
}

#[test]
fn test_failed_campaign_refund() {
    let t = setup();
    let sac = StellarAssetClient::new(&t.env, &t.token_addr);

    let owner = Address::generate(&t.env);
    let donor = Address::generate(&t.env);
    sac.mint(&donor, &10_000);

    let deadline = t.env.ledger().timestamp() + 1000;

    let cid = t.client.create_campaign(
        &owner,
        &5000i128,
        &t.token_addr,
        &deadline,
        &make_milestones(&t.env),
        &100i128,
    );

    advance_to_campaigning(&t, cid);

    // Pledge but not enough to fund
    t.client.pledge(&donor, &cid, &500);

    let balance_after_pledge = t.token.balance(&donor);

    // Advance past deadline
    t.env.ledger().with_mut(|l| {
        l.timestamp = deadline + 1;
    });

    // Mark as failed
    t.client.check_deadline(&cid);
    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Failed);

    // Backend supplies the backer list; contract verifies stored amounts.
    let mut backers = Vec::new(&t.env);
    backers.push_back((donor.clone(), 0i128)); // hint amount ignored; contract uses stored pledge
    t.client.process_refund_batch(&cid, &backers);

    // Donor got their pledge back
    assert_eq!(t.token.balance(&donor), balance_after_pledge + 500);
}

#[test]
fn test_cancel_campaign() {
    let t = setup();
    let sac = StellarAssetClient::new(&t.env, &t.token_addr);

    let owner = Address::generate(&t.env);
    let donor = Address::generate(&t.env);
    sac.mint(&donor, &5_000);

    let cid = t.client.create_campaign(
        &owner,
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    advance_to_campaigning(&t, cid);

    t.client.pledge(&donor, &cid, &200);

    // Admin cancels
    t.client.cancel_campaign(&cid);
    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Cancelled);

    // Process refund — backend supplies backer list
    let balance_before = t.token.balance(&donor);
    let mut backers = Vec::new(&t.env);
    backers.push_back((donor.clone(), 0i128));
    t.client.process_refund_batch(&cid, &backers);
    assert_eq!(t.token.balance(&donor), balance_before + 200);
}

#[test]
fn test_reject_milestone() {
    let t = setup();
    let sac = StellarAssetClient::new(&t.env, &t.token_addr);

    let owner = Address::generate(&t.env);
    let donor = Address::generate(&t.env);
    sac.mint(&donor, &10_000);

    let cid = t.client.create_campaign(
        &owner,
        &1000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    advance_to_campaigning(&t, cid);

    t.client.pledge(&donor, &cid, &1100);

    // Submit milestone 0
    t.client.submit_milestone(&cid, &0);

    // Reject it
    t.client.reject_milestone(&cid, &0);
    let ms = t.client.get_milestone(&cid, &0);
    assert_eq!(ms.status, CrowdfundMilestoneStatus::Rejected);

    // Can resubmit after rejection
    t.client.submit_milestone(&cid, &0);
    let ms = t.client.get_milestone(&cid, &0);
    assert_eq!(ms.status, CrowdfundMilestoneStatus::Submitted);
}

#[test]
fn test_invalid_milestones_rejected() {
    let t = setup();
    let owner = t.admin.clone();

    // Milestones that don't sum to 10000
    let mut bad_ms = Vec::new(&t.env);
    bad_ms.push_back(3000u32);
    bad_ms.push_back(3000u32);

    let result = t.client.try_create_campaign(
        &owner,
        &1000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &bad_ms,
        &100i128,
    );
    assert!(result.is_err());
}

#[test]
fn test_resolve_dispute_approve_creator() {
    let t = setup();
    let sac = StellarAssetClient::new(&t.env, &t.token_addr);

    let owner = Address::generate(&t.env);
    let donor = Address::generate(&t.env);
    sac.mint(&donor, &10_000);

    let cid = t.client.create_campaign(
        &owner,
        &1000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    advance_to_campaigning(&t, cid);

    // Fund the campaign
    t.client.pledge(&donor, &cid, &1100);
    assert_eq!(t.client.get_campaign(&cid).status, CampaignStatus::Funded);

    // Submit milestone 0
    t.client.submit_milestone(&cid, &0);
    assert_eq!(
        t.client.get_dispute_status(&cid, &0),
        CrowdfundMilestoneStatus::Submitted
    );

    // Backer disputes milestone 0
    t.client.dispute_milestone(&donor, &cid, &0);
    assert_eq!(
        t.client.get_dispute_status(&cid, &0),
        CrowdfundMilestoneStatus::Disputed
    );

    // Admin resolves in favor of creator → funds released
    let balance_before = t.token.balance(&owner);
    t.client
        .resolve_dispute(&cid, &0, &DisputeResolution::ApproveCreator);

    let ms = t.client.get_milestone(&cid, &0);
    assert_eq!(ms.status, CrowdfundMilestoneStatus::Released);
    assert!(t.token.balance(&owner) > balance_before);

    // Campaign is still Executing (milestone 1 not done yet)
    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Executing);

    // Complete milestone 1 normally
    t.client.submit_milestone(&cid, &1);
    t.client.approve_milestone(&cid, &1);

    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Completed);
}

#[test]
fn test_resolve_dispute_approve_backer() {
    let t = setup();
    let sac = StellarAssetClient::new(&t.env, &t.token_addr);

    let owner = Address::generate(&t.env);
    let donor = Address::generate(&t.env);
    sac.mint(&donor, &10_000);

    let cid = t.client.create_campaign(
        &owner,
        &1000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    advance_to_campaigning(&t, cid);

    // Fund the campaign
    t.client.pledge(&donor, &cid, &1100);

    // Submit and dispute milestone 0
    t.client.submit_milestone(&cid, &0);
    t.client.dispute_milestone(&donor, &cid, &0);

    // Admin resolves in favor of backer → milestone rejected, campaign cancelled
    t.client
        .resolve_dispute(&cid, &0, &DisputeResolution::ApproveBacker);

    let ms = t.client.get_milestone(&cid, &0);
    assert_eq!(ms.status, CrowdfundMilestoneStatus::Rejected);

    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Cancelled);

    // Backend supplies backer list for refund processing
    let balance_before_refund = t.token.balance(&donor);
    let mut backers = Vec::new(&t.env);
    backers.push_back((donor.clone(), 0i128));
    t.client.process_refund_batch(&cid, &backers);
    assert!(t.token.balance(&donor) > balance_before_refund);
}

#[test]
fn test_resolve_dispute_not_disputed_fails() {
    let t = setup();
    let sac = StellarAssetClient::new(&t.env, &t.token_addr);

    let owner = Address::generate(&t.env);
    let donor = Address::generate(&t.env);
    sac.mint(&donor, &10_000);

    let cid = t.client.create_campaign(
        &owner,
        &1000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    advance_to_campaigning(&t, cid);
    t.client.pledge(&donor, &cid, &1100);
    t.client.submit_milestone(&cid, &0);

    // Try to resolve a non-disputed milestone → should fail
    let result = t
        .client
        .try_resolve_dispute(&cid, &0, &DisputeResolution::ApproveCreator);
    assert!(result.is_err());
}

#[test]
fn test_vote_reject_cancels_campaign() {
    let t = setup();
    let owner = t.admin.clone();

    let cid = t.client.create_campaign(
        &owner,
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    // Admin approves → creates vote session (threshold=1)
    t.client.approve_campaign(&cid, &1000, &1);
    assert_eq!(
        t.client.get_campaign(&cid).status,
        CampaignStatus::Submitted
    );

    // Voter votes "Reject" (option 1)
    let voter = Address::generate(&t.env);
    t.client.vote_campaign(&voter, &cid, &1);

    // Check threshold → community rejected, campaign cancelled
    t.client.check_vote_threshold(&cid);

    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Cancelled);
    assert!(campaign.vote_session_id.is_none());
}

#[test]
fn test_vote_expired_without_quorum_cancels_campaign() {
    let t = setup();
    let owner = t.admin.clone();

    let cid = t.client.create_campaign(
        &owner,
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    // Admin approves → creates vote session (threshold=5, duration=1000)
    t.client.approve_campaign(&cid, &1000, &5);
    assert_eq!(
        t.client.get_campaign(&cid).status,
        CampaignStatus::Submitted
    );

    // Only 1 vote cast (threshold is 5), so threshold not reached
    let voter = Address::generate(&t.env);
    t.client.vote_campaign(&voter, &cid, &0);

    // Advance past voting deadline
    t.env.ledger().with_mut(|l| {
        l.timestamp += 1001;
    });

    // Check threshold → voting expired without quorum, campaign cancelled
    t.client.check_vote_threshold(&cid);

    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Cancelled);
    assert!(campaign.vote_session_id.is_none());
}

#[test]
fn test_vote_threshold_not_met_while_active() {
    let t = setup();
    let owner = t.admin.clone();

    let cid = t.client.create_campaign(
        &owner,
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    t.client.approve_campaign(&cid, &1000, &5);

    // No votes yet, voting still active → should error
    let result = t.client.try_check_vote_threshold(&cid);
    assert!(result.is_err());

    // Campaign stays in Submitted
    assert_eq!(
        t.client.get_campaign(&cid).status,
        CampaignStatus::Submitted
    );
}

#[test]
fn test_overdue_flag_and_escalate() {
    let t = setup();
    let sac = StellarAssetClient::new(&t.env, &t.token_addr);

    let owner = Address::generate(&t.env);
    let donor = Address::generate(&t.env);
    sac.mint(&donor, &10_000);

    let deadline = t.env.ledger().timestamp() + 5000;

    let cid = t.client.create_campaign(
        &owner,
        &1000i128,
        &t.token_addr,
        &deadline,
        &make_milestones(&t.env),
        &100i128,
    );

    advance_to_campaigning(&t, cid);
    t.client.pledge(&donor, &cid, &1100);

    // Advance 30+ days past deadline → flag overdue
    t.env.ledger().with_mut(|l| {
        l.timestamp = deadline + 30 * 86_400 + 1;
    });

    t.client.flag_overdue_milestone(&cid, &0);

    // Milestone should have flagged_at set
    let ms = t.client.get_milestone(&cid, &0);
    assert_eq!(ms.status, CrowdfundMilestoneStatus::Pending);
    assert!(ms.flagged_at > 0);

    // Escalate too early (before 14-day grace) → should fail
    let result = t.client.try_escalate_overdue_milestone(&cid, &0);
    assert!(result.is_err());

    // Advance 14+ days past flagging
    t.env.ledger().with_mut(|l| {
        l.timestamp += 14 * 86_400 + 1;
    });

    // Escalate now → should succeed
    t.client.escalate_overdue_milestone(&cid, &0);

    let ms = t.client.get_milestone(&cid, &0);
    assert_eq!(ms.status, CrowdfundMilestoneStatus::Rejected);

    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Cancelled);

    // Backend supplies backer list for refund
    let balance_before = t.token.balance(&donor);
    let mut backers = Vec::new(&t.env);
    backers.push_back((donor.clone(), 0i128));
    t.client.process_refund_batch(&cid, &backers);
    assert!(t.token.balance(&donor) > balance_before);
}

#[test]
fn test_overdue_escalate_not_flagged_fails() {
    let t = setup();
    let sac = StellarAssetClient::new(&t.env, &t.token_addr);

    let owner = Address::generate(&t.env);
    let donor = Address::generate(&t.env);
    sac.mint(&donor, &10_000);

    let deadline = t.env.ledger().timestamp() + 5000;

    let cid = t.client.create_campaign(
        &owner,
        &1000i128,
        &t.token_addr,
        &deadline,
        &make_milestones(&t.env),
        &100i128,
    );

    advance_to_campaigning(&t, cid);
    t.client.pledge(&donor, &cid, &1100);

    // Try to escalate without flagging first → should fail
    t.env.ledger().with_mut(|l| {
        l.timestamp = deadline + 60 * 86_400;
    });

    let result = t.client.try_escalate_overdue_milestone(&cid, &0);
    assert!(result.is_err());
}

#[test]
fn test_overdue_creator_submits_during_grace_period() {
    let t = setup();
    let sac = StellarAssetClient::new(&t.env, &t.token_addr);

    let owner = Address::generate(&t.env);
    let donor = Address::generate(&t.env);
    sac.mint(&donor, &10_000);

    let deadline = t.env.ledger().timestamp() + 5000;

    let cid = t.client.create_campaign(
        &owner,
        &1000i128,
        &t.token_addr,
        &deadline,
        &make_milestones(&t.env),
        &100i128,
    );

    advance_to_campaigning(&t, cid);
    t.client.pledge(&donor, &cid, &1100);

    // Flag overdue
    t.env.ledger().with_mut(|l| {
        l.timestamp = deadline + 30 * 86_400 + 1;
    });
    t.client.flag_overdue_milestone(&cid, &0);

    // Creator submits during grace period
    t.client.submit_milestone(&cid, &0);
    let ms = t.client.get_milestone(&cid, &0);
    assert_eq!(ms.status, CrowdfundMilestoneStatus::Submitted);

    // Advance past grace period
    t.env.ledger().with_mut(|l| {
        l.timestamp += 14 * 86_400 + 1;
    });

    // Escalate should fail — milestone is no longer Pending
    let result = t.client.try_escalate_overdue_milestone(&cid, &0);
    assert!(result.is_err());

    // Campaign still active
    assert_eq!(
        t.client.get_campaign(&cid).status,
        CampaignStatus::Executing
    );
}

#[test]
fn test_owner_cancel_in_submitted() {
    let t = setup();
    let owner = Address::generate(&t.env);

    let cid = t.client.create_campaign(
        &owner,
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    assert_eq!(
        t.client.get_campaign(&cid).status,
        CampaignStatus::Submitted
    );

    t.client.owner_cancel_campaign(&cid);
    assert_eq!(
        t.client.get_campaign(&cid).status,
        CampaignStatus::Cancelled
    );
}

#[test]
fn test_owner_cancel_in_campaigning_with_refunds() {
    let t = setup();
    let sac = StellarAssetClient::new(&t.env, &t.token_addr);

    let owner = Address::generate(&t.env);
    let donor = Address::generate(&t.env);
    sac.mint(&donor, &10_000);

    let cid = t.client.create_campaign(
        &owner,
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    advance_to_campaigning(&t, cid);

    // Backer pledges
    t.client.pledge(&donor, &cid, &500);
    let balance_after_pledge = t.token.balance(&donor);

    // Owner cancels
    t.client.owner_cancel_campaign(&cid);
    assert_eq!(
        t.client.get_campaign(&cid).status,
        CampaignStatus::Cancelled
    );

    // Refunds work — backend provides backer list
    let mut backers = Vec::new(&t.env);
    backers.push_back((donor.clone(), 0i128));
    t.client.process_refund_batch(&cid, &backers);
    assert_eq!(t.token.balance(&donor), balance_after_pledge + 500);
}

#[test]
fn test_owner_cancel_after_funded_fails() {
    let t = setup();
    let sac = StellarAssetClient::new(&t.env, &t.token_addr);

    let owner = Address::generate(&t.env);
    let donor = Address::generate(&t.env);
    sac.mint(&donor, &10_000);

    let cid = t.client.create_campaign(
        &owner,
        &1000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
    );

    advance_to_campaigning(&t, cid);
    t.client.pledge(&donor, &cid, &1100);
    assert_eq!(t.client.get_campaign(&cid).status, CampaignStatus::Funded);

    // Owner cannot cancel after funded
    let result = t.client.try_owner_cancel_campaign(&cid);
    assert!(result.is_err());
}
