/// End-to-end crowdfunding tests across CoreEscrow + ReputationRegistry + GovernanceVoting + CrowdfundRegistry.
/// Tests full campaign lifecycle: create → governance approval → pledge → fund → milestones → complete,
/// and failure path: pledge → deadline → batched refund.
use crate::setup::{setup_platform, Platform};
use crowdfund_registry::storage::CampaignStatus;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, String, Vec};

fn make_milestones(env: &soroban_sdk::Env) -> Vec<(String, u32)> {
    let mut ms = Vec::new(env);
    ms.push_back((String::from_str(env, "MVP"), 5000u32));
    ms.push_back((String::from_str(env, "Beta"), 5000u32));
    ms
}

/// Helper: advance a campaign from Draft → Submitted → Approved → Voted → Campaigning
fn advance_to_campaigning(p: &Platform, campaign_id: u64) {
    p.crowdfund.submit_for_review(&campaign_id);
    p.crowdfund.approve_campaign(&campaign_id, &1000, &1);
    let voter = Address::generate(&p.env);
    p.crowdfund.vote_campaign(&voter, &campaign_id, &0);
    p.crowdfund.check_vote_threshold(&campaign_id);
}

#[test]
fn test_campaign_full_success_lifecycle() {
    let p = setup_platform();
    let owner = Address::generate(&p.env);
    let backer1 = Address::generate(&p.env);
    let backer2 = Address::generate(&p.env);

    p.sac.mint(&backer1, &50_000);
    p.sac.mint(&backer2, &50_000);

    let cid = p.crowdfund.create_campaign(
        &owner,
        &String::from_str(&p.env, "Build a DAO tool"),
        &5_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &make_milestones(&p.env),
        &100i128,
        &false,
    );

    let campaign = p.crowdfund.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Draft);

    // Advance through governance flow
    advance_to_campaigning(&p, cid);
    assert_eq!(
        p.crowdfund.get_campaign(&cid).status,
        CampaignStatus::Campaigning
    );

    // Backers pledge (fee on top via CoreEscrow.route_pledge)
    p.crowdfund.pledge(&backer1, &cid, &3_000);
    let campaign = p.crowdfund.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Campaigning);

    // Second pledge pushes past goal → Funded
    p.crowdfund.pledge(&backer2, &cid, &3_000);
    let campaign = p.crowdfund.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Funded);

    // Treasury received pledge fees
    assert!(p.token.balance(&p.treasury) > 0);

    // Submit and approve milestones
    p.crowdfund.submit_milestone(&cid, &0);
    let campaign = p.crowdfund.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Executing);

    p.crowdfund.approve_milestone(&cid, &0);
    // Owner received partial payout (50% of funded amount)
    assert!(p.token.balance(&owner) > 0);

    p.crowdfund.submit_milestone(&cid, &1);
    p.crowdfund.approve_milestone(&cid, &1);

    let campaign = p.crowdfund.get_campaign(&cid);
    assert_eq!(campaign.status, CampaignStatus::Completed);
}

#[test]
fn test_campaign_failure_with_batched_refund() {
    let p = setup_platform();
    let owner = Address::generate(&p.env);
    let backer = Address::generate(&p.env);

    p.sac.mint(&backer, &50_000);

    let deadline = p.env.ledger().timestamp() + 5000;

    let cid = p.crowdfund.create_campaign(
        &owner,
        &String::from_str(&p.env, "Underfunded project"),
        &50_000i128,
        &p.token_addr,
        &deadline,
        &make_milestones(&p.env),
        &100i128,
        &false,
    );

    // Advance through governance flow
    advance_to_campaigning(&p, cid);

    // Pledge but far below goal
    p.crowdfund.pledge(&backer, &cid, &1_000);
    let backer_balance_after_pledge = p.token.balance(&backer);

    // Advance past deadline
    p.env.ledger().with_mut(|l| {
        l.timestamp = deadline + 1;
    });

    // Mark as failed (permissionless)
    p.crowdfund.check_deadline(&cid);
    assert_eq!(
        p.crowdfund.get_campaign(&cid).status,
        CampaignStatus::Failed
    );

    // Process refund batch (permissionless)
    p.crowdfund.process_refund_batch(&cid);

    // Backer got their pledge back
    assert_eq!(
        p.token.balance(&backer),
        backer_balance_after_pledge + 1_000
    );
}

#[test]
fn test_campaign_cancel_with_refund() {
    let p = setup_platform();
    let owner = Address::generate(&p.env);
    let backer = Address::generate(&p.env);

    p.sac.mint(&backer, &10_000);

    let cid = p.crowdfund.create_campaign(
        &owner,
        &String::from_str(&p.env, "Cancelled project"),
        &10_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &make_milestones(&p.env),
        &50i128,
        &false,
    );

    // Advance through governance flow
    advance_to_campaigning(&p, cid);

    p.crowdfund.pledge(&backer, &cid, &500);
    let backer_balance = p.token.balance(&backer);

    // Admin cancels
    p.crowdfund.cancel_campaign(&cid);
    assert_eq!(
        p.crowdfund.get_campaign(&cid).status,
        CampaignStatus::Cancelled
    );

    // Process refund
    p.crowdfund.process_refund_batch(&cid);
    assert_eq!(p.token.balance(&backer), backer_balance + 500);
}

#[test]
fn test_milestone_rejection_and_resubmit() {
    let p = setup_platform();
    let owner = Address::generate(&p.env);
    let backer = Address::generate(&p.env);

    p.sac.mint(&backer, &50_000);

    let cid = p.crowdfund.create_campaign(
        &owner,
        &String::from_str(&p.env, "Milestone revisions"),
        &2_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &make_milestones(&p.env),
        &100i128,
        &false,
    );

    // Advance through governance flow
    advance_to_campaigning(&p, cid);

    // Fund it
    p.crowdfund.pledge(&backer, &cid, &2_500);
    assert_eq!(
        p.crowdfund.get_campaign(&cid).status,
        CampaignStatus::Funded
    );

    // Submit milestone 0
    p.crowdfund.submit_milestone(&cid, &0);

    // Reject it
    p.crowdfund.reject_milestone(&cid, &0);
    let ms = p.crowdfund.get_milestone(&cid, &0);
    assert_eq!(
        ms.status,
        crowdfund_registry::storage::CrowdfundMilestoneStatus::Rejected
    );

    // Resubmit
    p.crowdfund.submit_milestone(&cid, &0);
    let ms = p.crowdfund.get_milestone(&cid, &0);
    assert_eq!(
        ms.status,
        crowdfund_registry::storage::CrowdfundMilestoneStatus::Submitted
    );

    // Approve and complete
    p.crowdfund.approve_milestone(&cid, &0);
    p.crowdfund.submit_milestone(&cid, &1);
    p.crowdfund.approve_milestone(&cid, &1);
    assert_eq!(
        p.crowdfund.get_campaign(&cid).status,
        CampaignStatus::Completed
    );
}

#[test]
fn test_pledge_fee_routing() {
    let p = setup_platform();
    let owner = Address::generate(&p.env);
    let backer = Address::generate(&p.env);

    p.sac.mint(&backer, &100_000);

    let cid = p.crowdfund.create_campaign(
        &owner,
        &String::from_str(&p.env, "Fee routing test"),
        &5_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &make_milestones(&p.env),
        &100i128,
        &false,
    );

    // Advance through governance flow
    advance_to_campaigning(&p, cid);

    let treasury_before = p.token.balance(&p.treasury);
    let insurance_before = p.escrow.get_insurance_balance();

    p.crowdfund.pledge(&backer, &cid, &1_000);

    // Crowdfund fee is 5% on top: 1000 pledge + 50 fee = 1050 total from backer
    // Fee split: 90% treasury (45), 10% insurance (5)
    let treasury_after = p.token.balance(&p.treasury);
    let insurance_after = p.escrow.get_insurance_balance();

    assert_eq!(treasury_after - treasury_before, 45);
    assert_eq!(insurance_after - insurance_before, 5);
}

#[test]
fn test_governance_approval_flow() {
    let p = setup_platform();
    let owner = Address::generate(&p.env);

    let cid = p.crowdfund.create_campaign(
        &owner,
        &String::from_str(&p.env, "Governance test"),
        &5_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &make_milestones(&p.env),
        &100i128,
        &false,
    );

    // Starts in Draft
    assert_eq!(p.crowdfund.get_campaign(&cid).status, CampaignStatus::Draft);

    // Submit for review → Submitted
    p.crowdfund.submit_for_review(&cid);
    assert_eq!(
        p.crowdfund.get_campaign(&cid).status,
        CampaignStatus::Submitted
    );

    // Admin approves → creates vote session (stays Submitted)
    let session_id = p.crowdfund.approve_campaign(&cid, &1000, &1);
    assert_eq!(
        p.crowdfund.get_campaign(&cid).status,
        CampaignStatus::Submitted
    );
    assert_eq!(p.crowdfund.get_vote_session(&cid), session_id);

    // Community votes
    let voter = Address::generate(&p.env);
    p.crowdfund.vote_campaign(&voter, &cid, &0);

    // Check threshold → Campaigning
    p.crowdfund.check_vote_threshold(&cid);
    assert_eq!(
        p.crowdfund.get_campaign(&cid).status,
        CampaignStatus::Campaigning
    );
}

#[test]
fn test_governance_rejection_flow() {
    let p = setup_platform();
    let owner = Address::generate(&p.env);

    let cid = p.crowdfund.create_campaign(
        &owner,
        &String::from_str(&p.env, "Rejected campaign"),
        &5_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &make_milestones(&p.env),
        &100i128,
        &false,
    );

    p.crowdfund.submit_for_review(&cid);

    // Admin rejects → back to Draft
    p.crowdfund
        .reject_campaign(&cid, &String::from_str(&p.env, "Needs more detail"));
    assert_eq!(p.crowdfund.get_campaign(&cid).status, CampaignStatus::Draft);

    // Owner can resubmit
    p.crowdfund.submit_for_review(&cid);
    assert_eq!(
        p.crowdfund.get_campaign(&cid).status,
        CampaignStatus::Submitted
    );
}
