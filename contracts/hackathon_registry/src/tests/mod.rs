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

    // Finalize (after judging deadline). With the pull model, finalize only
    // records winners — no token transfers happen here.
    t.env.ledger().set_timestamp(3500);

    let lead1_balance_before = t.token.balance(&lead1);
    let lead2_balance_before = t.token.balance(&lead2);

    t.client.finalize_hackathon(&hid);

    // Verify hackathon completed
    let hackathon = t.client.get_hackathon(&hid);
    assert_eq!(hackathon.status, HackathonStatus::Completed);

    // Balances should NOT have changed yet
    assert_eq!(t.token.balance(&lead1), lead1_balance_before);
    assert_eq!(t.token.balance(&lead2), lead2_balance_before);

    // Winner records exist with the correct amounts
    let w1 = t.client.get_winner_by_address(&hid, &lead1);
    assert_eq!(w1.rank, 0);
    assert_eq!(w1.amount, 6000); // 60% of 10000
    assert!(!w1.claimed);

    let w2 = t.client.get_winner_by_address(&hid, &lead2);
    assert_eq!(w2.rank, 1);
    assert_eq!(w2.amount, 4000); // 40% of 10000
    assert!(!w2.claimed);

    assert_eq!(t.client.get_winner_count(&hid), 2);

    // Winners pull their prizes
    let claimed1 = t.client.claim_prize(&hid, &lead1);
    let claimed2 = t.client.claim_prize(&hid, &lead2);
    assert_eq!(claimed1, 6000);
    assert_eq!(claimed2, 4000);

    let lead1_balance_after = t.token.balance(&lead1);
    let lead2_balance_after = t.token.balance(&lead2);
    assert_eq!(lead1_balance_after - lead1_balance_before, 6000);
    assert_eq!(lead2_balance_after - lead2_balance_before, 4000);

    // Re-claim should fail
    let result = t.client.try_claim_prize(&hid, &lead1);
    assert!(result.is_err(), "second claim should fail with AlreadyClaimed");

    // Verify reputation was recorded (unchanged behavior — happens at finalize)
    let profile1 = t.rep_client.get_profile(&lead1);
    assert!(profile1.hackathons_entered >= 1);
    assert!(profile1.hackathons_won >= 1);
    assert!(profile1.overall_score >= 100);

    let profile2 = t.rep_client.get_profile(&lead2);
    assert!(profile2.hackathons_entered >= 1);
    assert_eq!(profile2.hackathons_won, 0);
}

/// Verifies the reclaim path: a winner who never claims forfeits their prize
/// after the claim window expires, and the creator can sweep it back.
#[test]
fn test_reclaim_unclaimed_prizes() {
    let t = setup();
    let creator = t.admin.clone();

    let mut prize_tiers = Vec::new(&t.env);
    prize_tiers.push_back(10000u32); // 100% to top

    let hid = t.client.create_hackathon(
        &creator,
        &String::from_str(&t.env, "Reclaim Test"),
        &String::from_str(&t.env, "QmReclaim"),
        &10_000,
        &t.token_addr,
        &1000,
        &2000,
        &3000,
        &50,
        &prize_tiers,
    );

    let judge = Address::generate(&t.env);
    t.client.add_judge(&hid, &judge);

    let lead1 = Address::generate(&t.env);
    t.rep_client.init_profile(&lead1);

    t.env.ledger().set_timestamp(500);
    t.client.register_team(&hid, &lead1);

    t.env.ledger().set_timestamp(1500);
    t.client.submit_project(&hid, &lead1, &String::from_str(&t.env, "ipfs://r"));

    t.env.ledger().set_timestamp(2500);
    t.client.open_judging(&hid);
    t.client.score_submission(&hid, &judge, &lead1, &95);

    // Finalize. lead1 has a claimable prize but does not claim.
    t.env.ledger().set_timestamp(3500);
    let creator_balance_before = t.token.balance(&creator);
    let lead1_balance_before = t.token.balance(&lead1);

    t.client.finalize_hackathon(&hid);

    let winner = t.client.get_winner_by_address(&hid, &lead1);
    assert_eq!(winner.amount, 10_000);
    assert!(!winner.claimed);

    // Before claim window expires, reclaim should fail.
    let early = t.client.try_reclaim_unclaimed_prizes(&hid);
    assert!(
        early.is_err(),
        "reclaim before claim_deadline should be rejected"
    );
    assert_eq!(t.token.balance(&lead1), lead1_balance_before);

    // Fast-forward past the claim deadline (default 90 days).
    t.env
        .ledger()
        .set_timestamp(winner.claim_deadline + 1);

    let reclaimed = t.client.reclaim_unclaimed_prizes(&hid);
    assert_eq!(reclaimed, 10_000);

    // Creator gets the prize back.
    assert_eq!(
        t.token.balance(&creator) - creator_balance_before,
        10_000
    );
    // Winner never got anything.
    assert_eq!(t.token.balance(&lead1), lead1_balance_before);

    // After reclaim, the winner cannot claim anymore.
    let post = t.client.try_claim_prize(&hid, &lead1);
    assert!(post.is_err(), "claim after reclaim should fail");
}

/// Verifies that `create_and_fund_hackathon` is a behavioral alias for
/// `create_hackathon` — same atomicity, same balance impact.
#[test]
fn test_create_and_fund_hackathon_alias() {
    let t = setup();
    let creator = t.admin.clone();

    let mut prize_tiers = Vec::new(&t.env);
    prize_tiers.push_back(10000u32);

    let creator_balance_before = t.token.balance(&creator);

    let hid = t.client.create_and_fund_hackathon(
        &creator,
        &String::from_str(&t.env, "Aliased"),
        &String::from_str(&t.env, "QmAlias"),
        &7_500,
        &t.token_addr,
        &1000,
        &2000,
        &3000,
        &10,
        &prize_tiers,
    );

    assert_eq!(hid, 1);
    let hackathon = t.client.get_hackathon(&hid);
    assert_eq!(hackathon.prize_pool, 7_500);
    // Funds transferred atomically into escrow.
    assert_eq!(
        creator_balance_before - t.token.balance(&creator),
        7_500
    );
}

/// Verifies the admin-only `set_claim_window` guardrails.
#[test]
fn test_set_claim_window_bounds() {
    let t = setup();

    // Within bounds: 30 days.
    t.client.set_claim_window(&(30 * 86_400));
    assert_eq!(t.client.get_claim_window(), 30 * 86_400);

    // Too short — less than 1 day rejected.
    let too_short = t.client.try_set_claim_window(&3600);
    assert!(too_short.is_err());

    // Too long — more than 365 days rejected.
    let too_long = t.client.try_set_claim_window(&(400 * 86_400));
    assert!(too_long.is_err());
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

    // Pull model: finalize records the winner but does not transfer.
    assert_eq!(t.token.balance(&lead2), lead2_balance_before);

    // lead2 (next-best after disqualification) is the sole winner at rank 0
    let winner = t.client.get_winner_by_address(&hid, &lead2);
    assert_eq!(winner.rank, 0);
    assert_eq!(winner.amount, 10_000);

    // lead1 is not a winner record at all
    let dq_lookup = t.client.try_get_winner_by_address(&hid, &lead1);
    assert!(dq_lookup.is_err());

    // lead2 claims their prize
    let claimed = t.client.claim_prize(&hid, &lead2);
    assert_eq!(claimed, 10_000);
    assert_eq!(t.token.balance(&lead2) - lead2_balance_before, 10_000);

    // lead1 still has nothing
    assert_eq!(t.token.balance(&lead1), 0);
}
