#![cfg(test)]

use crate::contract::{HackathonRegistry, HackathonRegistryClient};
use crate::storage::HackathonStatus;
use core_escrow::{CoreEscrow, CoreEscrowClient};
use reputation_registry::{ReputationRegistry, ReputationRegistryClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Ledger;
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, Env, String, Vec};

struct TestEnv<'a> {
    env: Env,
    client: HackathonRegistryClient<'a>,
    _escrow_client: CoreEscrowClient<'a>,
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
    let token_addr = env
        .register_stellar_asset_contract_v2(token_admin)
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

    // Deploy HackathonRegistry
    let hack_id = env.register(HackathonRegistry, ());
    let client = HackathonRegistryClient::new(&env, &hack_id);
    client.init(&admin, &escrow_id, &rep_id);

    // Authorize HackathonRegistry in CoreEscrow and ReputationRegistry
    escrow_client.authorize_module(&hack_id);
    rep_client.add_authorized_module(&hack_id);

    // Mint tokens to admin for hackathon creation
    sac.mint(&admin, &1_000_000);

    TestEnv {
        env,
        client,
        _escrow_client: escrow_client,
        rep_client,
        admin,
        token,
        token_addr,
    }
}

#[test]
fn test_create_hackathon() {
    let t = setup();

    let creator = t.admin.clone();

    let mut prize_tiers = Vec::new(&t.env);
    prize_tiers.push_back(6000u32);
    prize_tiers.push_back(4000u32);

    let hid = t.client.create_hackathon(
        &creator,
        &String::from_str(&t.env, "Stellar Hackathon"),
        &String::from_str(&t.env, "QmHackMeta"),
        &10_000,
        &t.token_addr,
        &1000,
        &2000,
        &3000,
        &100,
        &prize_tiers,
    );

    assert_eq!(hid, 1);

    let hackathon = t.client.get_hackathon(&hid);
    assert_eq!(hackathon.id, 1);
    assert_eq!(hackathon.creator, creator);
    assert_eq!(hackathon.prize_pool, 10_000);
    assert_eq!(hackathon.status, HackathonStatus::Registration);
    assert_eq!(hackathon.max_participants, 100);
    assert_eq!(hackathon.judge_count, 0);
    assert_eq!(hackathon.submission_count, 0);
}

#[test]
fn test_full_lifecycle() {
    let t = setup();

    let creator = t.admin.clone();

    // Create hackathon
    let mut prize_tiers = Vec::new(&t.env);
    prize_tiers.push_back(6000u32);
    prize_tiers.push_back(4000u32);

    let hid = t.client.create_hackathon(
        &creator,
        &String::from_str(&t.env, "Stellar Hackathon"),
        &String::from_str(&t.env, "QmHackMeta"),
        &10_000,
        &t.token_addr,
        &1000, // registration deadline
        &2000, // submission deadline
        &3000, // judging deadline
        &100,
        &prize_tiers,
    );
    assert_eq!(hid, 1);

    // Add judges
    let judge1 = Address::generate(&t.env);
    let judge2 = Address::generate(&t.env);
    t.client.add_judge(&hid, &judge1);
    t.client.add_judge(&hid, &judge2);

    let hackathon = t.client.get_hackathon(&hid);
    assert_eq!(hackathon.judge_count, 2);

    // Register teams (spend credits)
    let lead1 = Address::generate(&t.env);
    let lead2 = Address::generate(&t.env);

    // Init profiles so they have credits
    t.rep_client.init_profile(&lead1);
    t.rep_client.init_profile(&lead2);

    t.env.ledger().set_timestamp(500); // before registration deadline

    t.client.register_team(&hid, &lead1);
    t.client.register_team(&hid, &lead2);

    let hackathon = t.client.get_hackathon(&hid);
    assert_eq!(hackathon.submission_count, 2);

    // Submit projects
    t.env.ledger().set_timestamp(1500); // between registration and submission deadlines

    t.client
        .submit_project(&hid, &lead1, &String::from_str(&t.env, "ipfs://project-a"));
    t.client
        .submit_project(&hid, &lead2, &String::from_str(&t.env, "ipfs://project-b"));

    // Open judging and score submissions (after submission deadline)
    t.env.ledger().set_timestamp(2500);
    t.client.open_judging(&hid);

    t.client.score_submission(&hid, &judge1, &lead1, &90);
    t.client.score_submission(&hid, &judge2, &lead1, &80);
    t.client.score_submission(&hid, &judge1, &lead2, &70);
    t.client.score_submission(&hid, &judge2, &lead2, &60);

    // Verify scores
    let sub1 = t.client.get_submission(&hid, &lead1);
    assert_eq!(sub1.total_score, 170); // 90 + 80
    assert_eq!(sub1.score_count, 2);

    let sub2 = t.client.get_submission(&hid, &lead2);
    assert_eq!(sub2.total_score, 130); // 70 + 60
    assert_eq!(sub2.score_count, 2);

    // Finalize (after judging deadline)
    t.env.ledger().set_timestamp(3500);

    let _creator_balance_before = t.token.balance(&creator);
    let lead1_balance_before = t.token.balance(&lead1);
    let lead2_balance_before = t.token.balance(&lead2);

    t.client.finalize_hackathon(&hid);

    // Verify hackathon completed
    let hackathon = t.client.get_hackathon(&hid);
    assert_eq!(hackathon.status, HackathonStatus::Completed);

    // Verify prize distribution
    // lead1 gets 60% of 10000 = 6000
    // lead2 gets 40% of 10000 = 4000
    let lead1_balance_after = t.token.balance(&lead1);
    let lead2_balance_after = t.token.balance(&lead2);

    assert_eq!(lead1_balance_after - lead1_balance_before, 6000);
    assert_eq!(lead2_balance_after - lead2_balance_before, 4000);

    // Verify reputation was recorded
    let profile1 = t.rep_client.get_profile(&lead1);
    assert!(profile1.hackathons_entered >= 1);
    assert!(profile1.hackathons_won >= 1);
    assert!(profile1.overall_score >= 100);

    let profile2 = t.rep_client.get_profile(&lead2);
    assert!(profile2.hackathons_entered >= 1);
    assert_eq!(profile2.hackathons_won, 0);
}

#[test]
fn test_cancel_hackathon() {
    let t = setup();

    let creator = t.admin.clone();

    let mut prize_tiers = Vec::new(&t.env);
    prize_tiers.push_back(6000u32);
    prize_tiers.push_back(4000u32);

    let creator_balance_before = t.token.balance(&creator);

    let hid = t.client.create_hackathon(
        &creator,
        &String::from_str(&t.env, "Cancel Me"),
        &String::from_str(&t.env, "QmCancel"),
        &10_000,
        &t.token_addr,
        &1000,
        &2000,
        &3000,
        &50,
        &prize_tiers,
    );

    // After creation, 10000 tokens transferred to escrow
    let creator_balance_after_create = t.token.balance(&creator);
    assert_eq!(
        creator_balance_before - creator_balance_after_create,
        10_000
    );

    // Cancel
    t.client.cancel_hackathon(&hid);

    // Verify refund
    let creator_balance_after_cancel = t.token.balance(&creator);
    assert_eq!(creator_balance_after_cancel, creator_balance_before);

    // Verify status
    let hackathon = t.client.get_hackathon(&hid);
    assert_eq!(hackathon.status, HackathonStatus::Cancelled);
}

#[test]
fn test_disqualify_submission() {
    let t = setup();

    let creator = t.admin.clone();

    let mut prize_tiers = Vec::new(&t.env);
    prize_tiers.push_back(10000u32); // 100% to winner

    let hid = t.client.create_hackathon(
        &creator,
        &String::from_str(&t.env, "DQ Test"),
        &String::from_str(&t.env, "QmDQ"),
        &10_000,
        &t.token_addr,
        &1000,
        &2000,
        &3000,
        &50,
        &prize_tiers,
    );

    // Add judge
    let judge = Address::generate(&t.env);
    t.client.add_judge(&hid, &judge);

    // Register teams
    let lead1 = Address::generate(&t.env);
    let lead2 = Address::generate(&t.env);
    t.rep_client.init_profile(&lead1);
    t.rep_client.init_profile(&lead2);

    t.env.ledger().set_timestamp(500);
    t.client.register_team(&hid, &lead1);
    t.client.register_team(&hid, &lead2);

    // Submit
    t.env.ledger().set_timestamp(1500);
    t.client
        .submit_project(&hid, &lead1, &String::from_str(&t.env, "ipfs://dq-a"));
    t.client
        .submit_project(&hid, &lead2, &String::from_str(&t.env, "ipfs://dq-b"));

    // Open judging and score - lead1 gets highest score
    t.env.ledger().set_timestamp(2500);
    t.client.open_judging(&hid);
    t.client.score_submission(&hid, &judge, &lead1, &95);
    t.client.score_submission(&hid, &judge, &lead2, &80);

    // Disqualify lead1 (the top scorer)
    t.client.disqualify_submission(&hid, &lead1);

    let sub1 = t.client.get_submission(&hid, &lead1);
    assert!(sub1.disqualified);

    // Finalize
    t.env.ledger().set_timestamp(3500);

    let lead2_balance_before = t.token.balance(&lead2);
    t.client.finalize_hackathon(&hid);

    // lead2 should get 100% since lead1 is disqualified
    let lead2_balance_after = t.token.balance(&lead2);
    assert_eq!(lead2_balance_after - lead2_balance_before, 10_000);

    // lead1 gets nothing
    let lead1_balance = t.token.balance(&lead1);
    assert_eq!(lead1_balance, 0);
}
