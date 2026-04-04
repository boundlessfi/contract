/// Edge case and security boundary tests:
/// - Crowdfund: request_milestone_revision, double-pledge, zero-amount edge
/// - Governance: double-vote prevention
/// - QF: multi-donor distribution verification
/// - Hackathon: double-registration prevention
use crate::setup::{setup_platform, Platform};
use crowdfund_registry::storage::{CampaignStatus, CrowdfundMilestoneStatus};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, String, Vec};

// ============================================================================
// HELPERS
// ============================================================================

fn make_milestones(env: &soroban_sdk::Env) -> Vec<(String, u32)> {
    let mut ms = Vec::new(env);
    ms.push_back((String::from_str(env, "MVP"), 5000u32));
    ms.push_back((String::from_str(env, "Beta"), 5000u32));
    ms
}

fn advance_to_campaigning(p: &Platform, campaign_id: u64) {
    p.crowdfund.submit_for_review(&campaign_id);
    p.crowdfund.approve_campaign(&campaign_id, &1000, &1);
    let voter = Address::generate(&p.env);
    p.crowdfund.vote_campaign(&voter, &campaign_id, &0);
    p.crowdfund.check_vote_threshold(&campaign_id);
}

// ============================================================================
// CROWDFUND: request_milestone_revision
// ============================================================================

#[test]
fn test_request_milestone_revision() {
    let p = setup_platform();
    let owner = Address::generate(&p.env);
    let backer = Address::generate(&p.env);
    p.sac.mint(&backer, &50_000);

    let cid = p.crowdfund.create_campaign(
        &owner,
        &String::from_str(&p.env, "Revision test"),
        &2_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &make_milestones(&p.env),
        &100i128,
        &false,
    );

    advance_to_campaigning(&p, cid);

    // Fund
    p.crowdfund.pledge(&backer, &cid, &3_000);
    assert_eq!(
        p.crowdfund.get_campaign(&cid).status,
        CampaignStatus::Funded
    );

    // Submit milestone
    p.crowdfund.submit_milestone(&cid, &0);
    assert_eq!(
        p.crowdfund.get_milestone(&cid, &0).status,
        CrowdfundMilestoneStatus::Submitted
    );

    // Admin requests revision → back to Pending
    p.crowdfund.request_milestone_revision(&cid, &0);
    assert_eq!(
        p.crowdfund.get_milestone(&cid, &0).status,
        CrowdfundMilestoneStatus::Pending
    );

    // Owner can resubmit
    p.crowdfund.submit_milestone(&cid, &0);
    assert_eq!(
        p.crowdfund.get_milestone(&cid, &0).status,
        CrowdfundMilestoneStatus::Submitted
    );

    // Complete the full lifecycle
    p.crowdfund.approve_milestone(&cid, &0);
    p.crowdfund.submit_milestone(&cid, &1);
    p.crowdfund.approve_milestone(&cid, &1);
    assert_eq!(
        p.crowdfund.get_campaign(&cid).status,
        CampaignStatus::Completed
    );
}

#[test]
fn test_revision_on_disputed_milestone() {
    let p = setup_platform();
    let owner = Address::generate(&p.env);
    let backer = Address::generate(&p.env);
    p.sac.mint(&backer, &50_000);

    let cid = p.crowdfund.create_campaign(
        &owner,
        &String::from_str(&p.env, "Disputed revision"),
        &2_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &make_milestones(&p.env),
        &100i128,
        &false,
    );

    advance_to_campaigning(&p, cid);
    p.crowdfund.pledge(&backer, &cid, &3_000);

    // Submit → Dispute → Request revision (back to Pending)
    p.crowdfund.submit_milestone(&cid, &0);
    p.crowdfund.dispute_milestone(&backer, &cid, &0);
    assert_eq!(
        p.crowdfund.get_milestone(&cid, &0).status,
        CrowdfundMilestoneStatus::Disputed
    );

    p.crowdfund.request_milestone_revision(&cid, &0);
    assert_eq!(
        p.crowdfund.get_milestone(&cid, &0).status,
        CrowdfundMilestoneStatus::Pending
    );
}

// ============================================================================
// CROWDFUND: double pledge by same backer (should accumulate)
// ============================================================================

#[test]
fn test_double_pledge_accumulates() {
    let p = setup_platform();
    let owner = Address::generate(&p.env);
    let backer = Address::generate(&p.env);
    p.sac.mint(&backer, &50_000);

    let cid = p.crowdfund.create_campaign(
        &owner,
        &String::from_str(&p.env, "Double pledge"),
        &5_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &make_milestones(&p.env),
        &100i128,
        &false,
    );

    advance_to_campaigning(&p, cid);

    p.crowdfund.pledge(&backer, &cid, &500);
    let pledge1 = p.crowdfund.get_pledge(&cid, &backer);
    assert!(pledge1 > 0);

    // Second pledge by same backer
    p.crowdfund.pledge(&backer, &cid, &500);
    let pledge2 = p.crowdfund.get_pledge(&cid, &backer);
    assert!(pledge2 > pledge1); // accumulated

    // Backer count should still be 1
    let campaign = p.crowdfund.get_campaign(&cid);
    assert_eq!(campaign.backer_count, 1);
}

// ============================================================================
// GOVERNANCE: double-vote prevention
// ============================================================================

#[test]
#[should_panic]
fn test_governance_double_vote_rejected() {
    let p = setup_platform();
    let owner = Address::generate(&p.env);

    let cid = p.crowdfund.create_campaign(
        &owner,
        &String::from_str(&p.env, "Double vote test"),
        &5_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &make_milestones(&p.env),
        &100i128,
        &false,
    );

    p.crowdfund.submit_for_review(&cid);
    p.crowdfund.approve_campaign(&cid, &1000, &1);

    let voter = Address::generate(&p.env);
    p.crowdfund.vote_campaign(&voter, &cid, &0);

    // Same voter tries to vote again → should panic
    p.crowdfund.vote_campaign(&voter, &cid, &0);
}

// ============================================================================
// CROWDFUND: pledge below minimum rejected
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #806)")]
fn test_pledge_below_minimum_rejected() {
    let p = setup_platform();
    let owner = Address::generate(&p.env);
    let backer = Address::generate(&p.env);
    p.sac.mint(&backer, &50_000);

    let cid = p.crowdfund.create_campaign(
        &owner,
        &String::from_str(&p.env, "Min pledge test"),
        &5_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &make_milestones(&p.env),
        &500i128, // min pledge = 500
        &false,
    );

    advance_to_campaigning(&p, cid);

    // Pledge below minimum → should fail
    p.crowdfund.pledge(&backer, &cid, &100);
}

// ============================================================================
// QF: multi-donor distribution
// ============================================================================

#[test]
fn test_qf_multi_donor_distribution() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);
    p.sac.mint(&creator, &100_000);

    let mut project_names: Vec<String> = Vec::new(&p.env);
    project_names.push_back(String::from_str(&p.env, "Project A"));
    project_names.push_back(String::from_str(&p.env, "Project B"));
    project_names.push_back(String::from_str(&p.env, "Project C"));

    let gid = p
        .grant
        .create_qf_round(&creator, &30_000, &p.token_addr, &project_names, &2_592_000);

    // Multiple donors contribute to different projects
    // Project A: 4 small donations (QF: sum of sqrt is larger than sqrt of sum)
    p.grant.donate_to_project(&gid, &25, &0);
    p.grant.donate_to_project(&gid, &25, &0);
    p.grant.donate_to_project(&gid, &25, &0);
    p.grant.donate_to_project(&gid, &25, &0);
    // sum_sqrt_A = 4 * sqrt(25*10^6) = 4 * 5000 = 20000
    // squared = 400_000_000

    // Project B: one large donation of same total
    p.grant.donate_to_project(&gid, &100, &1);
    // sum_sqrt_B = sqrt(100*10^6) = 10000
    // squared = 100_000_000

    // Project C: two medium donations
    p.grant.donate_to_project(&gid, &50, &2);
    p.grant.donate_to_project(&gid, &50, &2);
    // sum_sqrt_C = 2 * sqrt(50*10^6) = 2 * 7071 = 14142
    // squared = 200_008_164

    // Advance past session
    p.env
        .ledger()
        .set_timestamp(p.env.ledger().timestamp() + 3_000_000);

    let proj_a = Address::generate(&p.env);
    let proj_b = Address::generate(&p.env);
    let proj_c = Address::generate(&p.env);
    let mut addrs: Vec<Address> = Vec::new(&p.env);
    addrs.push_back(proj_a.clone());
    addrs.push_back(proj_b.clone());
    addrs.push_back(proj_c.clone());

    p.grant.finalize_qf_round(&gid, &addrs);

    let bal_a = p.token.balance(&proj_a);
    let bal_b = p.token.balance(&proj_b);
    let bal_c = p.token.balance(&proj_c);

    // All projects should receive some matching funds
    assert!(bal_a > 0);
    assert!(bal_b > 0);
    assert!(bal_c > 0);

    // Total should equal matching pool (within rounding tolerance of integer division)
    let total = bal_a + bal_b + bal_c;
    assert!(
        (29_990..=30_000).contains(&total),
        "Total {total} should be ~30000"
    );

    // QF favors many small donors: Project A (3 donors × 100)
    // should get more than Project B (1 donor × 900)
    // even though total donated amounts are similar
    assert!(
        bal_a > bal_b,
        "QF should favor many small donors over fewer large ones"
    );
}

// ============================================================================
// HACKATHON: double-registration prevention
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #1007)")]
fn test_hackathon_double_registration_rejected() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);
    let lead = Address::generate(&p.env);

    p.sac.mint(&creator, &100_000);
    p.reputation.init_profile(&lead);

    let mut prize_tiers = Vec::new(&p.env);
    prize_tiers.push_back(10000u32);

    let hid = p.hackathon.create_hackathon(
        &creator,
        &String::from_str(&p.env, "No double reg"),
        &String::from_str(&p.env, "Qm"),
        &10_000,
        &p.token_addr,
        &1000,
        &2000,
        &3000,
        &50,
        &prize_tiers,
    );

    p.env.ledger().set_timestamp(500);
    p.hackathon.register_team(&hid, &lead);

    // Second registration → should fail
    p.hackathon.register_team(&hid, &lead);
}

// ============================================================================
// CROWDFUND: cannot pledge in Draft status
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #805)")]
fn test_pledge_in_draft_rejected() {
    let p = setup_platform();
    let owner = Address::generate(&p.env);
    let backer = Address::generate(&p.env);
    p.sac.mint(&backer, &50_000);

    let cid = p.crowdfund.create_campaign(
        &owner,
        &String::from_str(&p.env, "Draft pledge attempt"),
        &5_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &make_milestones(&p.env),
        &100i128,
        &false,
    );

    // Campaign is in Draft — pledge should fail
    p.crowdfund.pledge(&backer, &cid, &500);
}
