/// End-to-end bounty tests across CoreEscrow + ReputationRegistry + BountyRegistry.
/// Tests all 4 bounty sub-types through their full lifecycle.
use crate::setup::setup_platform;
use boundless_types::ActivityCategory;
use bounty_registry::storage::{ApplicationStatus, BountyStatus, BountyType};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, String};

#[test]
fn test_fcfs_full_lifecycle() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);
    let contributor = Address::generate(&p.env);

    p.sac.mint(&creator, &50_000);
    p.reputation.init_profile(&contributor);

    // Create FCFS bounty
    let bounty_id = p.bounty.create_bounty(
        &creator,
        &String::from_str(&p.env, "Fix login bug"),
        &String::from_str(&p.env, "QmABC"),
        &BountyType::FCFS,
        &10_000,
        &p.token_addr,
        &ActivityCategory::Development,
        &(p.env.ledger().timestamp() + 86400),
    );

    // Verify escrow pool was funded
    let bounty = p.bounty.get_bounty(&bounty_id);
    assert_eq!(bounty.status, BountyStatus::Open);
    assert!(p.escrow.get_pool(&bounty.escrow_pool_id).total_deposited > 0);

    // Contributor claims (spends 1 credit)
    assert_eq!(p.reputation.get_credits(&contributor), 3);
    p.bounty.claim_bounty(&contributor, &bounty_id);
    assert_eq!(p.reputation.get_credits(&contributor), 2);

    let bounty = p.bounty.get_bounty(&bounty_id);
    assert_eq!(bounty.status, BountyStatus::InProgress);

    // Creator approves with rating
    p.bounty.approve_fcfs(&creator, &bounty_id, &80);

    // Verify completion
    let bounty = p.bounty.get_bounty(&bounty_id);
    assert_eq!(bounty.status, BountyStatus::Completed);

    // Contributor received payout
    assert!(p.token.balance(&contributor) > 0);

    // Reputation was updated
    let profile = p.reputation.get_profile(&contributor);
    assert_eq!(profile.bounties_completed, 1);
    assert!(profile.overall_score > 0);
}

#[test]
fn test_application_flow_with_credit_restore() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);
    let app1 = Address::generate(&p.env);
    let app2 = Address::generate(&p.env);
    let app3 = Address::generate(&p.env);

    p.sac.mint(&creator, &50_000);
    p.reputation.init_profile(&app1);
    p.reputation.init_profile(&app2);
    p.reputation.init_profile(&app3);

    let bounty_id = p.bounty.create_bounty(
        &creator,
        &String::from_str(&p.env, "Design system"),
        &String::from_str(&p.env, "QmDesign"),
        &BountyType::Application,
        &5_000,
        &p.token_addr,
        &ActivityCategory::Design,
        &(p.env.ledger().timestamp() + 86400),
    );

    // All 3 apply (each spends 1 credit: 3 → 2)
    p.bounty
        .apply(&app1, &bounty_id, &String::from_str(&p.env, "Proposal A"));
    p.bounty
        .apply(&app2, &bounty_id, &String::from_str(&p.env, "Proposal B"));
    p.bounty
        .apply(&app3, &bounty_id, &String::from_str(&p.env, "Proposal C"));
    assert_eq!(p.reputation.get_credits(&app1), 2);
    assert_eq!(p.reputation.get_credits(&app2), 2);
    assert_eq!(p.reputation.get_credits(&app3), 2);

    // Select app1 → app2 and app3 get credits restored
    p.bounty.select_applicant(&creator, &bounty_id, &app1);
    assert_eq!(p.reputation.get_credits(&app2), 3);
    assert_eq!(p.reputation.get_credits(&app3), 3);
    assert_eq!(p.reputation.get_credits(&app1), 2); // selected, not restored

    // app1 submits and gets approved
    p.bounty
        .submit_work(&app1, &bounty_id, &String::from_str(&p.env, "QmWork"));
    p.bounty.approve_submission(&creator, &bounty_id, &90);

    let bounty = p.bounty.get_bounty(&bounty_id);
    assert_eq!(bounty.status, BountyStatus::Completed);
    assert!(p.token.balance(&app1) > 0);
    assert_eq!(p.reputation.get_profile(&app1).bounties_completed, 1);
}

#[test]
fn test_contest_multi_winner() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);
    let sub1 = Address::generate(&p.env);
    let sub2 = Address::generate(&p.env);

    p.sac.mint(&creator, &50_000);
    p.reputation.init_profile(&sub1);
    p.reputation.init_profile(&sub2);

    let bounty_id = p.bounty.create_bounty(
        &creator,
        &String::from_str(&p.env, "Design contest"),
        &String::from_str(&p.env, "QmContest"),
        &BountyType::Contest,
        &10_000,
        &p.token_addr,
        &ActivityCategory::Design,
        &(p.env.ledger().timestamp() + 86400),
    );

    // Pool is locked at creation for contests
    let bounty = p.bounty.get_bounty(&bounty_id);
    assert!(p.escrow.is_locked(&bounty.escrow_pool_id));

    // Submit work
    p.bounty
        .submit_work(&sub1, &bounty_id, &String::from_str(&p.env, "QmW1"));
    p.bounty
        .submit_work(&sub2, &bounty_id, &String::from_str(&p.env, "QmW2"));

    // Pick winners with split prizes
    p.bounty
        .approve_contest_winner(&creator, &bounty_id, &sub1, &6000, &90);
    p.bounty
        .approve_contest_winner(&creator, &bounty_id, &sub2, &4000, &70);
    p.bounty.finalize_contest(&creator, &bounty_id);

    assert_eq!(
        p.bounty.get_bounty(&bounty_id).status,
        BountyStatus::Completed
    );
    assert_eq!(p.token.balance(&sub1), 6000);
    assert_eq!(p.token.balance(&sub2), 4000);

    // Both got reputation
    assert_eq!(p.reputation.get_profile(&sub1).bounties_completed, 1);
    assert_eq!(p.reputation.get_profile(&sub2).bounties_completed, 1);
}

#[test]
fn test_split_bounty_multi_contributor() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);
    let dev1 = Address::generate(&p.env);
    let dev2 = Address::generate(&p.env);

    p.sac.mint(&creator, &50_000);
    p.reputation.init_profile(&dev1);
    p.reputation.init_profile(&dev2);

    let bounty_id = p.bounty.create_bounty(
        &creator,
        &String::from_str(&p.env, "Multi-part build"),
        &String::from_str(&p.env, "QmSplit"),
        &BountyType::Split,
        &8_000,
        &p.token_addr,
        &ActivityCategory::Development,
        &(p.env.ledger().timestamp() + 86400),
    );

    // Define splits
    let mut slots = soroban_sdk::Vec::new(&p.env);
    slots.push_back((dev1.clone(), 5_000i128));
    slots.push_back((dev2.clone(), 3_000i128));
    p.bounty.define_splits(&creator, &bounty_id, &slots);

    // Approve each
    p.bounty.approve_split(&creator, &bounty_id, &0, &85);
    assert_eq!(p.token.balance(&dev1), 5_000);

    p.bounty.approve_split(&creator, &bounty_id, &1, &75);
    assert_eq!(p.token.balance(&dev2), 3_000);

    // Both got reputation
    assert_eq!(p.reputation.get_profile(&dev1).bounties_completed, 1);
    assert_eq!(p.reputation.get_profile(&dev2).bounties_completed, 1);
}

#[test]
fn test_cancel_bounty_refunds_escrow_and_credits() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);
    let applicant = Address::generate(&p.env);

    p.sac.mint(&creator, &50_000);
    p.reputation.init_profile(&applicant);

    let bounty_id = p.bounty.create_bounty(
        &creator,
        &String::from_str(&p.env, "Will cancel"),
        &String::from_str(&p.env, "QmC"),
        &BountyType::Application,
        &5_000,
        &p.token_addr,
        &ActivityCategory::Community,
        &(p.env.ledger().timestamp() + 86400),
    );

    let creator_balance_after_create = p.token.balance(&creator);

    // Applicant applies (credit spent: 3 → 2)
    p.bounty.apply(
        &applicant,
        &bounty_id,
        &String::from_str(&p.env, "Proposal"),
    );
    assert_eq!(p.reputation.get_credits(&applicant), 2);

    // Cancel bounty
    p.bounty.cancel_bounty(&creator, &bounty_id);

    // Credit restored
    assert_eq!(p.reputation.get_credits(&applicant), 3);

    // Creator got escrow refund
    assert!(p.token.balance(&creator) > creator_balance_after_create);

    // Status is cancelled
    assert_eq!(
        p.bounty.get_bounty(&bounty_id).status,
        BountyStatus::Cancelled
    );
}

#[test]
fn test_reject_application_restores_credit() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);
    let applicant = Address::generate(&p.env);

    p.sac.mint(&creator, &50_000);
    p.reputation.init_profile(&applicant);

    let bounty_id = p.bounty.create_bounty(
        &creator,
        &String::from_str(&p.env, "Task"),
        &String::from_str(&p.env, "Qm"),
        &BountyType::Application,
        &1_000,
        &p.token_addr,
        &ActivityCategory::Security,
        &(p.env.ledger().timestamp() + 86400),
    );

    p.bounty
        .apply(&applicant, &bounty_id, &String::from_str(&p.env, "Prop"));
    assert_eq!(p.reputation.get_credits(&applicant), 2);

    p.bounty
        .reject_application(&creator, &bounty_id, &applicant);
    assert_eq!(p.reputation.get_credits(&applicant), 3);

    let app = p.bounty.get_application(&bounty_id, &applicant);
    assert_eq!(app.status, ApplicationStatus::Rejected);
}

#[test]
fn test_bounty_escrow_pool_created_correctly() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);

    p.sac.mint(&creator, &100_000);

    let bounty_id = p.bounty.create_bounty(
        &creator,
        &String::from_str(&p.env, "Escrow test"),
        &String::from_str(&p.env, "QmEscrow"),
        &BountyType::FCFS,
        &10_000,
        &p.token_addr,
        &ActivityCategory::Development,
        &(p.env.ledger().timestamp() + 86400),
    );

    // Verify escrow pool was created with the full bounty amount
    let bounty = p.bounty.get_bounty(&bounty_id);
    let pool = p.escrow.get_pool(&bounty.escrow_pool_id);
    assert_eq!(pool.total_deposited, 10_000);
    assert_eq!(pool.total_released, 0);
    assert_eq!(pool.total_refunded, 0);

    // Creator's tokens moved to escrow
    assert_eq!(p.token.balance(&creator), 90_000);
    assert_eq!(p.token.balance(&p.escrow_addr), 10_000);
}
