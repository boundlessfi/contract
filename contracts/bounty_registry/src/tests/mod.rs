#![cfg(test)]

use crate::contract::{BountyRegistry, BountyRegistryClient};
use crate::storage::{BountyStatus, BountyType};
use boundless_types::ActivityCategory;
use core_escrow::{CoreEscrow, CoreEscrowClient};
use reputation_registry::{ReputationRegistry, ReputationRegistryClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, Env, String};

struct TestEnv<'a> {
    env: Env,
    bounty_client: BountyRegistryClient<'a>,
    escrow_client: CoreEscrowClient<'a>,
    rep_client: ReputationRegistryClient<'a>,
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
    let token_addr = env.register_stellar_asset_contract_v2(token_admin).address();
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

    // Deploy BountyRegistry
    let bounty_id = env.register(BountyRegistry, ());
    let bounty_client = BountyRegistryClient::new(&env, &bounty_id);
    bounty_client.init(&admin, &escrow_id, &rep_id);

    // Authorize BountyRegistry in CoreEscrow and ReputationRegistry
    escrow_client.authorize_module(&bounty_id);
    rep_client.add_authorized_module(&bounty_id);

    // Mint tokens to admin for bounty creation
    sac.mint(&admin, &100_000);

    TestEnv {
        env,
        bounty_client,
        escrow_client,
        rep_client,
        admin,
        token,
        token_addr,
    }
}

#[test]
fn test_create_bounty() {
    let t = setup();

    let creator = t.admin.clone();
    let bounty_id = t.bounty_client.create_bounty(
        &creator,
        &String::from_str(&t.env, "Fix login bug"),
        &String::from_str(&t.env, "QmABC123"),
        &BountyType::Application,
        &1000,
        &t.token_addr,
        &ActivityCategory::Development,
        &(t.env.ledger().timestamp() + 86400),
    );

    assert_eq!(bounty_id, 1);
    let bounty = t.bounty_client.get_bounty(&1);
    assert_eq!(bounty.status, BountyStatus::Open);
    assert_eq!(bounty.amount, 1000);
    assert_eq!(bounty.bounty_type, BountyType::Application);
}

#[test]
fn test_fcfs_flow() {
    let t = setup();

    let creator = t.admin.clone();
    let contributor = Address::generate(&t.env);

    // Init contributor profile (gives 3 credits)
    t.rep_client.init_profile(&contributor);

    let bounty_id = t.bounty_client.create_bounty(
        &creator,
        &String::from_str(&t.env, "Quick task"),
        &String::from_str(&t.env, "Qm123"),
        &BountyType::FCFS,
        &500,
        &t.token_addr,
        &ActivityCategory::Development,
        &(t.env.ledger().timestamp() + 86400),
    );

    // Contributor claims
    t.bounty_client.claim_bounty(&contributor, &bounty_id);

    let bounty = t.bounty_client.get_bounty(&bounty_id);
    assert_eq!(bounty.status, BountyStatus::InProgress);
    assert_eq!(bounty.assignee, Some(contributor.clone()));

    // Credits reduced by 1
    assert_eq!(t.rep_client.get_credits(&contributor), 2);

    // Creator approves
    t.bounty_client.approve_fcfs(&creator, &bounty_id, &50);

    let bounty = t.bounty_client.get_bounty(&bounty_id);
    assert_eq!(bounty.status, BountyStatus::Completed);

    // Contributor got tokens
    assert_eq!(t.token.balance(&contributor), 500);

    // Profile updated
    let profile = t.rep_client.get_profile(&contributor);
    assert_eq!(profile.overall_score, 50);
    assert_eq!(profile.bounties_completed, 1);
}

#[test]
fn test_application_flow() {
    let t = setup();

    let creator = t.admin.clone();
    let applicant1 = Address::generate(&t.env);
    let applicant2 = Address::generate(&t.env);

    t.rep_client.init_profile(&applicant1);
    t.rep_client.init_profile(&applicant2);

    let bounty_id = t.bounty_client.create_bounty(
        &creator,
        &String::from_str(&t.env, "Design logo"),
        &String::from_str(&t.env, "QmDesign"),
        &BountyType::Application,
        &2000,
        &t.token_addr,
        &ActivityCategory::Design,
        &(t.env.ledger().timestamp() + 86400),
    );

    // Both apply (each spends 1 credit)
    t.bounty_client.apply(
        &applicant1,
        &bounty_id,
        &String::from_str(&t.env, "My proposal A"),
    );
    t.bounty_client.apply(
        &applicant2,
        &bounty_id,
        &String::from_str(&t.env, "My proposal B"),
    );

    assert_eq!(t.rep_client.get_credits(&applicant1), 2);
    assert_eq!(t.rep_client.get_credits(&applicant2), 2);

    // Creator selects applicant1 → applicant2 gets credit restored
    t.bounty_client
        .select_applicant(&creator, &bounty_id, &applicant1);

    assert_eq!(t.rep_client.get_credits(&applicant2), 3); // restored

    let bounty = t.bounty_client.get_bounty(&bounty_id);
    assert_eq!(bounty.status, BountyStatus::InProgress);
    assert_eq!(bounty.assignee, Some(applicant1.clone()));

    // Applicant1 submits work
    t.bounty_client.submit_work(
        &applicant1,
        &bounty_id,
        &String::from_str(&t.env, "QmWork"),
    );

    let bounty = t.bounty_client.get_bounty(&bounty_id);
    assert_eq!(bounty.status, BountyStatus::InReview);

    // Creator approves
    t.bounty_client
        .approve_submission(&creator, &bounty_id, &100);

    let bounty = t.bounty_client.get_bounty(&bounty_id);
    assert_eq!(bounty.status, BountyStatus::Completed);
    assert_eq!(t.token.balance(&applicant1), 2000);
}

#[test]
fn test_contest_flow() {
    let t = setup();

    let creator = t.admin.clone();
    let sub1 = Address::generate(&t.env);
    let sub2 = Address::generate(&t.env);

    let bounty_id = t.bounty_client.create_bounty(
        &creator,
        &String::from_str(&t.env, "Best design contest"),
        &String::from_str(&t.env, "QmContest"),
        &BountyType::Contest,
        &3000,
        &t.token_addr,
        &ActivityCategory::Design,
        &(t.env.ledger().timestamp() + 86400),
    );

    // Pool should be locked (Contest locks at creation)
    assert!(t.escrow_client.is_locked(
        &t.bounty_client.get_bounty(&bounty_id).escrow_pool_id
    ));

    // Init profiles for reputation recording
    t.rep_client.init_profile(&sub1);
    t.rep_client.init_profile(&sub2);

    // Both submit work
    t.bounty_client.submit_work(
        &sub1,
        &bounty_id,
        &String::from_str(&t.env, "QmWork1"),
    );
    t.bounty_client.submit_work(
        &sub2,
        &bounty_id,
        &String::from_str(&t.env, "QmWork2"),
    );

    // Creator picks sub1 as winner with 2000, sub2 with 1000
    t.bounty_client.approve_contest_winner(
        &creator,
        &bounty_id,
        &sub1,
        &2000,
        &80,
    );
    t.bounty_client.approve_contest_winner(
        &creator,
        &bounty_id,
        &sub2,
        &1000,
        &40,
    );

    assert_eq!(t.token.balance(&sub1), 2000);
    assert_eq!(t.token.balance(&sub2), 1000);

    // Finalize
    t.bounty_client.finalize_contest(&creator, &bounty_id);
    let bounty = t.bounty_client.get_bounty(&bounty_id);
    assert_eq!(bounty.status, BountyStatus::Completed);
    assert_eq!(bounty.winner_count, 2);
}

#[test]
fn test_split_flow() {
    let t = setup();

    let creator = t.admin.clone();
    let dev1 = Address::generate(&t.env);
    let dev2 = Address::generate(&t.env);

    t.rep_client.init_profile(&dev1);
    t.rep_client.init_profile(&dev2);

    let bounty_id = t.bounty_client.create_bounty(
        &creator,
        &String::from_str(&t.env, "Multi-part task"),
        &String::from_str(&t.env, "QmSplit"),
        &BountyType::Split,
        &4000,
        &t.token_addr,
        &ActivityCategory::Development,
        &(t.env.ledger().timestamp() + 86400),
    );

    // Define splits: dev1 gets 2500, dev2 gets 1500
    let mut slots = soroban_sdk::Vec::new(&t.env);
    slots.push_back((dev1.clone(), 2500i128));
    slots.push_back((dev2.clone(), 1500i128));
    t.bounty_client.define_splits(&creator, &bounty_id, &slots);

    // Approve each split
    t.bounty_client.approve_split(&creator, &bounty_id, &0, &60);
    assert_eq!(t.token.balance(&dev1), 2500);

    t.bounty_client.approve_split(&creator, &bounty_id, &1, &40);
    assert_eq!(t.token.balance(&dev2), 1500);

    // Both got reputation
    assert_eq!(t.rep_client.get_profile(&dev1).bounties_completed, 1);
    assert_eq!(t.rep_client.get_profile(&dev2).bounties_completed, 1);
}

#[test]
fn test_cancel_bounty_restores_credits() {
    let t = setup();

    let creator = t.admin.clone();
    let applicant = Address::generate(&t.env);
    t.rep_client.init_profile(&applicant);

    let bounty_id = t.bounty_client.create_bounty(
        &creator,
        &String::from_str(&t.env, "Cancelled task"),
        &String::from_str(&t.env, "QmCancel"),
        &BountyType::Application,
        &1000,
        &t.token_addr,
        &ActivityCategory::Community,
        &(t.env.ledger().timestamp() + 86400),
    );

    t.bounty_client.apply(
        &applicant,
        &bounty_id,
        &String::from_str(&t.env, "My proposal"),
    );
    assert_eq!(t.rep_client.get_credits(&applicant), 2);

    // Cancel → credit restored
    t.bounty_client.cancel_bounty(&creator, &bounty_id);

    assert_eq!(t.rep_client.get_credits(&applicant), 3);

    let bounty = t.bounty_client.get_bounty(&bounty_id);
    assert_eq!(bounty.status, BountyStatus::Cancelled);

    // Creator got tokens back
    assert_eq!(t.token.balance(&creator), 100_000);
}

#[test]
fn test_reject_application_restores_credit() {
    let t = setup();

    let creator = t.admin.clone();
    let applicant = Address::generate(&t.env);
    t.rep_client.init_profile(&applicant);

    let bounty_id = t.bounty_client.create_bounty(
        &creator,
        &String::from_str(&t.env, "Some task"),
        &String::from_str(&t.env, "Qm"),
        &BountyType::Application,
        &500,
        &t.token_addr,
        &ActivityCategory::Marketing,
        &(t.env.ledger().timestamp() + 86400),
    );

    t.bounty_client.apply(
        &applicant,
        &bounty_id,
        &String::from_str(&t.env, "proposal"),
    );
    assert_eq!(t.rep_client.get_credits(&applicant), 2);

    t.bounty_client
        .reject_application(&creator, &bounty_id, &applicant);

    assert_eq!(t.rep_client.get_credits(&applicant), 3);

    let app = t.bounty_client.get_application(&bounty_id, &applicant);
    assert_eq!(
        app.status,
        crate::storage::ApplicationStatus::Rejected
    );
}
