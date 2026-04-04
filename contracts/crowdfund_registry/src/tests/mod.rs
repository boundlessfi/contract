use crate::contract::{CrowdfundRegistry, CrowdfundRegistryClient};
use crate::storage::{CampaignStatus, CrowdfundMilestoneStatus, DisputeResolution};
use core_escrow::{CoreEscrow, CoreEscrowClient};
use governance_voting::{GovernanceVoting, GovernanceVotingClient};
use reputation_registry::{ReputationRegistry, ReputationRegistryClient};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, Env, String, Vec};

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

fn make_milestones(env: &Env) -> Vec<(String, u32)> {
    let mut ms = Vec::new(env);
    ms.push_back((String::from_str(env, "MVP"), 5000u32));
    ms.push_back((String::from_str(env, "Beta"), 5000u32));
    ms
}

/// Helper: advance a campaign from Draft → Submitted → Approved (with vote session) → vote → Campaigning
fn advance_to_campaigning(t: &TestEnv, campaign_id: u64) {
    // Owner submits for review
    t.client.submit_for_review(&campaign_id);

    // Admin approves (creates voting session with duration=1000, threshold=1)
    let _session_id = t.client.approve_campaign(&campaign_id, &1000, &1);

    // A voter votes (option 0 = "Approve")
    let voter = Address::generate(&t.env);
    t.client.vote_campaign(&voter, &campaign_id, &0);

    // Check threshold → transitions to Campaigning
    t.client.check_vote_threshold(&campaign_id);
}

#[test]
fn test_create_campaign() {
    let t = setup();
    let owner = t.admin.clone();

    let cid = t.client.create_campaign(
        &owner,
        &String::from_str(&t.env, "My Campaign"),
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
        &false,
    );

    assert_eq!(cid, 1);
    let campaign = t.client.get_campaign(&1);
    assert_eq!(campaign.status, CampaignStatus::Draft);
    assert_eq!(campaign.funding_goal, 10000);
    assert_eq!(campaign.milestone_count, 2);
}

#[test]
fn test_governance_flow() {
    let t = setup();
    let owner = t.admin.clone();

    let cid = t.client.create_campaign(
        &owner,
        &String::from_str(&t.env, "Gov flow"),
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
        &false,
    );

    assert_eq!(t.client.get_campaign(&cid).status, CampaignStatus::Draft);

    // Submit for review
    t.client.submit_for_review(&cid);
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
        &String::from_str(&t.env, "Rejected"),
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
        &false,
    );

    t.client.submit_for_review(&cid);
    t.client.reject_campaign(&cid, &String::from_str(&t.env, "Need more detail"));
    assert_eq!(t.client.get_campaign(&cid).status, CampaignStatus::Draft);
}

#[test]
fn test_create_and_submit_campaign() {
    let t = setup();
    let owner = t.admin.clone();

    let cid = t.client.create_campaign(
        &owner,
        &String::from_str(&t.env, "Instant Submit"),
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
        &true,
    );

    assert_eq!(t.client.get_campaign(&cid).status, CampaignStatus::Submitted);
}

#[test]
fn test_update_campaign() {
    let t = setup();
    let owner = t.admin.clone();

    let cid = t.client.create_campaign(
        &owner,
        &String::from_str(&t.env, "Draft"),
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
        &false,
    );

    let new_goal = 20000i128;
    let mut new_ms = Vec::new(&t.env);
    new_ms.push_back((String::from_str(&t.env, "Phase 1"), 5000u32));
    new_ms.push_back((String::from_str(&t.env, "Phase 2"), 5000u32));

    t.client.update_campaign(
        &cid,
        &String::from_str(&t.env, "Updated"),
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
        &String::from_str(&t.env, "Build a DAO"),
        &1000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
        &false,
    );

    // Advance through governance flow
    advance_to_campaigning(&t, cid);

    // Pledge enough to fund (fee-on-top: backers pay more than the pledge)
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
        &String::from_str(&t.env, "Underfunded"),
        &5000i128,
        &t.token_addr,
        &deadline,
        &make_milestones(&t.env),
        &100i128,
        &false,
    );

    // Advance through governance flow
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

    // Process refund batch
    t.client.process_refund_batch(&cid);

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
        &String::from_str(&t.env, "Cancel me"),
        &10000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
        &false,
    );

    // Advance through governance flow
    advance_to_campaigning(&t, cid);

    t.client.pledge(&donor, &cid, &200);

    // Admin cancels
    t.client.cancel_campaign(&cid);
    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Cancelled);

    // Process refund
    let balance_before = t.token.balance(&donor);
    t.client.process_refund_batch(&cid);
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
        &String::from_str(&t.env, "With rejection"),
        &1000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
        &false,
    );

    // Advance through governance flow
    advance_to_campaigning(&t, cid);

    t.client.pledge(&donor, &cid, &1100);

    // Submit milestone 0
    t.client.submit_milestone(&cid, &0);

    // Reject it
    t.client.reject_milestone(&cid, &0);
    let ms = t.client.get_milestone(&cid, &0);
    assert_eq!(
        ms.status,
        crate::storage::CrowdfundMilestoneStatus::Rejected
    );

    // Can resubmit after rejection
    t.client.submit_milestone(&cid, &0);
    let ms = t.client.get_milestone(&cid, &0);
    assert_eq!(
        ms.status,
        crate::storage::CrowdfundMilestoneStatus::Submitted
    );
}

#[test]
fn test_invalid_milestones_rejected() {
    let t = setup();
    let owner = t.admin.clone();

    // Milestones that don't sum to 10000
    let mut bad_ms = Vec::new(&t.env);
    bad_ms.push_back((String::from_str(&t.env, "A"), 3000u32));
    bad_ms.push_back((String::from_str(&t.env, "B"), 3000u32));

    let result = t.client.try_create_campaign(
        &owner,
        &String::from_str(&t.env, "Bad"),
        &1000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &bad_ms,
        &100i128,
        &false,
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
        &String::from_str(&t.env, "Dispute Creator Win"),
        &1000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
        &false,
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
        &String::from_str(&t.env, "Dispute Backer Win"),
        &1000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
        &false,
    );

    advance_to_campaigning(&t, cid);

    // Fund the campaign
    t.client.pledge(&donor, &cid, &1100);

    // Submit and dispute milestone 0
    t.client.submit_milestone(&cid, &0);
    t.client.dispute_milestone(&donor, &cid, &0);

    // Admin resolves in favor of backer → milestone rejected, campaign cancelled
    let balance_before_refund = t.token.balance(&donor);
    t.client
        .resolve_dispute(&cid, &0, &DisputeResolution::ApproveBacker);

    let ms = t.client.get_milestone(&cid, &0);
    assert_eq!(ms.status, CrowdfundMilestoneStatus::Rejected);

    let campaign = t.client.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Cancelled);

    // Backers can now get refunds
    t.client.process_refund_batch(&cid);
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
        &String::from_str(&t.env, "Not disputed"),
        &1000i128,
        &t.token_addr,
        &(t.env.ledger().timestamp() + 86400),
        &make_milestones(&t.env),
        &100i128,
        &false,
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
