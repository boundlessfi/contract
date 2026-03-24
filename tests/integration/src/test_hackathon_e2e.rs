/// End-to-end hackathon tests across CoreEscrow + ReputationRegistry + HackathonRegistry.
/// Tests full lifecycle: create → register → submit → judge → finalize → distribute prizes.
use crate::setup::setup_platform;
use hackathon_registry::storage::HackathonStatus;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, String, Vec};

#[test]
fn test_hackathon_full_lifecycle() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);
    let judge1 = Address::generate(&p.env);
    let judge2 = Address::generate(&p.env);
    let lead1 = Address::generate(&p.env);
    let lead2 = Address::generate(&p.env);

    p.sac.mint(&creator, &100_000);
    p.reputation.init_profile(&lead1);
    p.reputation.init_profile(&lead2);

    // Create hackathon with 60/40 prize split
    let mut prize_tiers = Vec::new(&p.env);
    prize_tiers.push_back(6000u32);
    prize_tiers.push_back(4000u32);

    let hid = p.hackathon.create_hackathon(
        &creator,
        &String::from_str(&p.env, "Stellar Hackathon 2026"),
        &String::from_str(&p.env, "QmHackMeta"),
        &10_000,
        &p.token_addr,
        &1000, // registration deadline
        &2000, // submission deadline
        &3000, // judging deadline
        &100,
        &prize_tiers,
    );

    let hackathon = p.hackathon.get_hackathon(&hid);
    assert_eq!(hackathon.status, HackathonStatus::Registration);
    assert_eq!(hackathon.prize_pool, 10_000);

    // Add judges
    p.hackathon.add_judge(&hid, &judge1);
    p.hackathon.add_judge(&hid, &judge2);
    assert_eq!(p.hackathon.get_hackathon(&hid).judge_count, 2);

    // Register teams (before registration deadline)
    p.env.ledger().set_timestamp(500);
    p.hackathon.register_team(&hid, &lead1);
    p.hackathon.register_team(&hid, &lead2);

    // SparkCredits are NOT spent for hackathon registration (credits are bounty-only)
    assert_eq!(p.reputation.get_credits(&lead1), 3);
    assert_eq!(p.reputation.get_credits(&lead2), 3);

    assert_eq!(p.hackathon.get_hackathon(&hid).submission_count, 2);

    // Submit projects (between registration and submission deadlines)
    p.env.ledger().set_timestamp(1500);
    p.hackathon
        .submit_project(&hid, &lead1, &String::from_str(&p.env, "ipfs://project-a"));
    p.hackathon
        .submit_project(&hid, &lead2, &String::from_str(&p.env, "ipfs://project-b"));

    // Score submissions (after submission deadline)
    p.env.ledger().set_timestamp(2500);
    p.hackathon.score_submission(&hid, &judge1, &lead1, &90);
    p.hackathon.score_submission(&hid, &judge2, &lead1, &80);
    p.hackathon.score_submission(&hid, &judge1, &lead2, &70);
    p.hackathon.score_submission(&hid, &judge2, &lead2, &60);

    // Verify aggregate scores
    let sub1 = p.hackathon.get_submission(&hid, &lead1);
    assert_eq!(sub1.total_score, 170);
    assert_eq!(sub1.score_count, 2);

    let sub2 = p.hackathon.get_submission(&hid, &lead2);
    assert_eq!(sub2.total_score, 130);
    assert_eq!(sub2.score_count, 2);

    // Finalize (after judging deadline)
    p.env.ledger().set_timestamp(3500);

    let lead1_before = p.token.balance(&lead1);
    let lead2_before = p.token.balance(&lead2);

    p.hackathon.finalize_hackathon(&hid);

    assert_eq!(
        p.hackathon.get_hackathon(&hid).status,
        HackathonStatus::Completed
    );

    // Prize distribution: lead1 wins (60%), lead2 runner-up (40%)
    assert_eq!(p.token.balance(&lead1) - lead1_before, 6000);
    assert_eq!(p.token.balance(&lead2) - lead2_before, 4000);

    // Reputation updated
    let profile1 = p.reputation.get_profile(&lead1);
    assert!(profile1.hackathons_entered >= 1);
    assert!(profile1.hackathons_won >= 1);
    assert!(profile1.overall_score >= 100);

    let profile2 = p.reputation.get_profile(&lead2);
    assert!(profile2.hackathons_entered >= 1);
    assert_eq!(profile2.hackathons_won, 0);
}

#[test]
fn test_hackathon_cancel_refunds_creator() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);

    p.sac.mint(&creator, &100_000);
    let balance_before = p.token.balance(&creator);

    let mut prize_tiers = Vec::new(&p.env);
    prize_tiers.push_back(10000u32);

    let hid = p.hackathon.create_hackathon(
        &creator,
        &String::from_str(&p.env, "Cancel Me"),
        &String::from_str(&p.env, "QmCancel"),
        &10_000,
        &p.token_addr,
        &1000,
        &2000,
        &3000,
        &50,
        &prize_tiers,
    );

    // 10k moved to escrow
    assert_eq!(p.token.balance(&creator), balance_before - 10_000);

    p.hackathon.cancel_hackathon(&hid);
    assert_eq!(
        p.hackathon.get_hackathon(&hid).status,
        HackathonStatus::Cancelled
    );

    // Full refund
    assert_eq!(p.token.balance(&creator), balance_before);
}

#[test]
fn test_disqualify_shifts_prizes() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);
    let judge = Address::generate(&p.env);
    let lead1 = Address::generate(&p.env);
    let lead2 = Address::generate(&p.env);

    p.sac.mint(&creator, &100_000);
    p.reputation.init_profile(&lead1);
    p.reputation.init_profile(&lead2);

    let mut prize_tiers = Vec::new(&p.env);
    prize_tiers.push_back(10000u32); // 100% to winner

    let hid = p.hackathon.create_hackathon(
        &creator,
        &String::from_str(&p.env, "DQ Test"),
        &String::from_str(&p.env, "QmDQ"),
        &10_000,
        &p.token_addr,
        &1000,
        &2000,
        &3000,
        &50,
        &prize_tiers,
    );

    p.hackathon.add_judge(&hid, &judge);

    // Register and submit
    p.env.ledger().set_timestamp(500);
    p.hackathon.register_team(&hid, &lead1);
    p.hackathon.register_team(&hid, &lead2);

    p.env.ledger().set_timestamp(1500);
    p.hackathon
        .submit_project(&hid, &lead1, &String::from_str(&p.env, "ipfs://a"));
    p.hackathon
        .submit_project(&hid, &lead2, &String::from_str(&p.env, "ipfs://b"));

    // Score (lead1 scores higher)
    p.env.ledger().set_timestamp(2500);
    p.hackathon.score_submission(&hid, &judge, &lead1, &95);
    p.hackathon.score_submission(&hid, &judge, &lead2, &80);

    // Disqualify the top scorer
    p.hackathon.disqualify_submission(&hid, &lead1);
    assert!(p.hackathon.get_submission(&hid, &lead1).disqualified);

    // Finalize
    p.env.ledger().set_timestamp(3500);
    p.hackathon.finalize_hackathon(&hid);

    // lead2 gets 100% since lead1 disqualified
    assert_eq!(p.token.balance(&lead2), 10_000);
    assert_eq!(p.token.balance(&lead1), 0);
}

#[test]
fn test_hackathon_escrow_pool() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);

    p.sac.mint(&creator, &100_000);

    let mut prize_tiers = Vec::new(&p.env);
    prize_tiers.push_back(10000u32);

    let hid = p.hackathon.create_hackathon(
        &creator,
        &String::from_str(&p.env, "Pool test"),
        &String::from_str(&p.env, "QmPool"),
        &10_000,
        &p.token_addr,
        &1000,
        &2000,
        &3000,
        &50,
        &prize_tiers,
    );

    // Full prize pool deposited to escrow
    let hackathon = p.hackathon.get_hackathon(&hid);
    let pool = p.escrow.get_pool(&hackathon.pool_id);
    assert_eq!(pool.total_deposited, 10_000);
    assert_eq!(p.token.balance(&creator), 90_000);
    assert_eq!(p.token.balance(&p.escrow_addr), 10_000);
}
