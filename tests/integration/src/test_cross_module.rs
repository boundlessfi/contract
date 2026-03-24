/// Cross-module integration test: a single contributor participates across all 4 modules
/// and we verify unified reputation, SparkCredits, and fee accounting.
use crate::setup::{setup_platform, Platform};
use boundless_types::ActivityCategory;
use bounty_registry::storage::BountyType;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, String, Vec};

/// Helper: advance a campaign from Draft → Campaigning via governance flow
fn advance_to_campaigning(p: &Platform, campaign_id: u64) {
    p.crowdfund.submit_for_review(&campaign_id);
    p.crowdfund.approve_campaign(&campaign_id, &1000, &1);
    let voter = Address::generate(&p.env);
    p.crowdfund.vote_campaign(&voter, &campaign_id, &0);
    p.crowdfund.check_vote_threshold(&campaign_id);
}

#[test]
fn test_single_contributor_across_all_modules() {
    let p = setup_platform();

    let admin_funder = Address::generate(&p.env);
    let contributor = Address::generate(&p.env);

    p.sac.mint(&admin_funder, &500_000);
    p.reputation.init_profile(&contributor);

    // Starting state: 3 credits, 0 score
    assert_eq!(p.reputation.get_credits(&contributor), 3);
    let profile = p.reputation.get_profile(&contributor);
    assert_eq!(profile.overall_score, 0);
    assert_eq!(profile.bounties_completed, 0);
    assert_eq!(profile.hackathons_entered, 0);

    // =========================================
    // Module 1: Complete a bounty (FCFS)
    // =========================================
    let bounty_id = p.bounty.create_bounty(
        &admin_funder,
        &String::from_str(&p.env, "Cross-module bounty"),
        &String::from_str(&p.env, "QmB"),
        &BountyType::FCFS,
        &5_000,
        &p.token_addr,
        &ActivityCategory::Development,
        &(p.env.ledger().timestamp() + 86400),
    );

    p.bounty.claim_bounty(&contributor, &bounty_id);
    assert_eq!(p.reputation.get_credits(&contributor), 2); // spent 1

    p.bounty.approve_fcfs(&admin_funder, &bounty_id, &80);

    let profile = p.reputation.get_profile(&contributor);
    assert_eq!(profile.bounties_completed, 1);
    assert!(profile.overall_score > 0);
    let score_after_bounty = profile.overall_score;

    // =========================================
    // Module 2: Back a crowdfunding campaign
    // =========================================
    p.sac.mint(&contributor, &10_000);

    let mut milestones = Vec::new(&p.env);
    milestones.push_back((String::from_str(&p.env, "Phase 1"), 5000u32));
    milestones.push_back((String::from_str(&p.env, "Phase 2"), 5000u32));

    let campaign_owner = Address::generate(&p.env);

    let cid = p.crowdfund.create_campaign(
        &campaign_owner,
        &String::from_str(&p.env, "Cross-module campaign"),
        &2_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &milestones,
        &100i128,
    );

    advance_to_campaigning(&p, cid);

    p.crowdfund.pledge(&contributor, &cid, &2_500);

    // Pledge was recorded (campaign may not be fully funded yet)
    let pledge_amount = p.crowdfund.get_pledge(&cid, &contributor);
    assert!(pledge_amount > 0);

    // =========================================
    // Module 3: Participate in a hackathon
    // =========================================
    let hack_creator = Address::generate(&p.env);
    let judge = Address::generate(&p.env);

    p.sac.mint(&hack_creator, &100_000);

    let mut prize_tiers = Vec::new(&p.env);
    prize_tiers.push_back(10000u32);

    let hid = p.hackathon.create_hackathon(
        &hack_creator,
        &String::from_str(&p.env, "Cross-module hack"),
        &String::from_str(&p.env, "QmH"),
        &10_000,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 1000),
        &(p.env.ledger().timestamp() + 2000),
        &(p.env.ledger().timestamp() + 3000),
        &100,
        &prize_tiers,
    );

    p.hackathon.add_judge(&hid, &judge);

    // Register (no credit spent — SparkCredits are bounty-only)
    p.hackathon.register_team(&hid, &contributor);

    // Submit
    p.env.ledger().with_mut(|l| {
        l.timestamp += 1500;
    });
    p.hackathon.submit_project(
        &hid,
        &contributor,
        &String::from_str(&p.env, "ipfs://cross"),
    );

    // Open judging and score
    p.env.ledger().with_mut(|l| {
        l.timestamp += 600;
    });
    p.hackathon.open_judging(&hid);
    p.hackathon
        .score_submission(&hid, &judge, &contributor, &85);

    // Finalize (only 1 participant → gets 100%)
    p.env.ledger().with_mut(|l| {
        l.timestamp += 1000;
    });
    p.hackathon.finalize_hackathon(&hid);

    let profile = p.reputation.get_profile(&contributor);
    assert!(profile.hackathons_entered >= 1);
    assert!(profile.hackathons_won >= 1);
    assert!(profile.overall_score > score_after_bounty);

    // =========================================
    // Verify unified state
    // =========================================
    let final_profile = p.reputation.get_profile(&contributor);
    assert_eq!(final_profile.bounties_completed, 1);
    assert!(final_profile.hackathons_entered >= 1);
    assert!(final_profile.overall_score > 0);

    // Credits: 3 (start) - 1 (bounty claim) + 1 (bounty completion award) = 3
    // Hackathon registration does NOT spend credits (SparkCredits are bounty-only)
    assert_eq!(p.reputation.get_credits(&contributor), 3);
}

#[test]
fn test_platform_fee_accounting_via_pledges() {
    let p = setup_platform();

    let backer = Address::generate(&p.env);
    let campaign_owner = Address::generate(&p.env);
    p.sac.mint(&backer, &100_000);

    let mut milestones = Vec::new(&p.env);
    milestones.push_back((String::from_str(&p.env, "M1"), 5000u32));
    milestones.push_back((String::from_str(&p.env, "M2"), 5000u32));

    let cid = p.crowdfund.create_campaign(
        &campaign_owner,
        &String::from_str(&p.env, "Fee accounting"),
        &50_000i128,
        &p.token_addr,
        &(p.env.ledger().timestamp() + 86400),
        &milestones,
        &100i128,
    );

    advance_to_campaigning(&p, cid);

    let treasury_start = p.token.balance(&p.treasury);
    let insurance_start = p.escrow.get_insurance_balance();

    // Pledge 10,000 (fee-on-top: 5% = 500. Treasury 450, insurance 50)
    p.crowdfund.pledge(&backer, &cid, &10_000);

    let treasury_end = p.token.balance(&p.treasury);
    let insurance_end = p.escrow.get_insurance_balance();

    assert_eq!(treasury_end - treasury_start, 450);
    assert_eq!(insurance_end - insurance_start, 50);
}

#[test]
fn test_spark_credit_recharge() {
    let p = setup_platform();
    let user = Address::generate(&p.env);

    p.reputation.init_profile(&user);
    assert_eq!(p.reputation.get_credits(&user), 3);

    // Use a credit by creating a bounty and applying
    let funder = Address::generate(&p.env);
    p.sac.mint(&funder, &100_000);

    let bounty_id = p.bounty.create_bounty(
        &funder,
        &String::from_str(&p.env, "Credit test"),
        &String::from_str(&p.env, "Qm"),
        &BountyType::FCFS,
        &1_000,
        &p.token_addr,
        &ActivityCategory::Development,
        &(p.env.ledger().timestamp() + 86400),
    );

    p.bounty.claim_bounty(&user, &bounty_id);
    assert_eq!(p.reputation.get_credits(&user), 2);

    // Advance 14 days and recharge
    p.env.ledger().with_mut(|l| {
        l.timestamp += 1_209_600 + 1; // 14 days + 1 second
    });

    p.reputation.try_recharge(&user);

    // Recharged +3, capped at max (10). Was 2, now 5.
    assert_eq!(p.reputation.get_credits(&user), 5);
}

#[test]
fn test_all_contracts_wired_correctly() {
    let p = setup_platform();

    // Verify admin addresses match
    assert_eq!(p.escrow.get_admin(), p.admin);

    // Verify treasury
    assert_eq!(p.escrow.get_treasury(), p.treasury);

    // Verify insurance starts at 0
    assert_eq!(p.escrow.get_insurance_balance(), 0);

    // Verify fee config defaults
    assert_eq!(
        p.escrow.get_fee_rate(&boundless_types::SubType::BountyFCFS),
        500
    );
    assert_eq!(
        p.escrow
            .get_fee_rate(&boundless_types::SubType::CrowdfundPledge),
        500
    );
    assert_eq!(
        p.escrow
            .get_fee_rate(&boundless_types::SubType::GrantMilestone),
        300
    );
    assert_eq!(
        p.escrow
            .get_fee_rate(&boundless_types::SubType::HackathonMain),
        400
    );
}
