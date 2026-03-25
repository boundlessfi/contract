use crate::contract::{GrantHub, GrantHubClient};
use crate::storage::{GrantMilestoneStatus, GrantStatus, GrantType};
use core_escrow::{CoreEscrow, CoreEscrowClient};
use governance_voting::{GovernanceVoting, GovernanceVotingClient};
use reputation_registry::{ReputationRegistry, ReputationRegistryClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, Env, String, Vec};

#[allow(dead_code)]
struct TestEnv<'a> {
    env: Env,
    hub_client: GrantHubClient<'a>,
    escrow_client: CoreEscrowClient<'a>,
    rep_client: ReputationRegistryClient<'a>,
    gov_client: GovernanceVotingClient<'a>,
    admin: Address,
    token: TokenClient<'a>,
    token_addr: Address,
    escrow_id: Address,
    gov_id: Address,
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

    // Deploy GovernanceVoting
    let gov_id = env.register(GovernanceVoting, ());
    let gov_client = GovernanceVotingClient::new(&env, &gov_id);
    gov_client.init(&admin);

    // Deploy GrantHub
    let hub_id = env.register(GrantHub, ());
    let hub_client = GrantHubClient::new(&env, &hub_id);
    hub_client.init(&admin, &escrow_id, &rep_id, &gov_id);

    // Authorize GrantHub in all dependent contracts
    escrow_client.authorize_module(&hub_id);
    rep_client.add_authorized_module(&hub_id);
    gov_client.add_authorized_module(&hub_id);

    // Mint tokens to admin
    sac.mint(&admin, &1_000_000);

    TestEnv {
        env,
        hub_client,
        escrow_client,
        rep_client,
        gov_client,
        admin,
        token,
        token_addr,
        escrow_id,
        gov_id,
    }
}

#[test]
fn test_milestone_grant_lifecycle() {
    let t = setup();

    let creator = t.admin.clone();
    let recipient = Address::generate(&t.env);

    // Mint tokens to creator
    StellarAssetClient::new(&t.env, &t.token_addr).mint(&creator, &10_000);

    // Create milestones: 60% first, 40% second
    let mut milestone_descs: Vec<(String, u32)> = Vec::new(&t.env);
    milestone_descs.push_back((String::from_str(&t.env, "Phase 1 delivery"), 6000));
    milestone_descs.push_back((String::from_str(&t.env, "Phase 2 delivery"), 4000));

    let grant_id = t.hub_client.create_milestone_grant(
        &creator,
        &recipient,
        &10_000,
        &t.token_addr,
        &milestone_descs,
    );

    assert_eq!(grant_id, 1);

    // Verify funds moved to escrow
    assert_eq!(t.token.balance(&t.escrow_id), 10_000);

    let grant = t.hub_client.get_grant(&grant_id);
    assert_eq!(grant.status, GrantStatus::Active);
    assert_eq!(grant.grant_type, GrantType::Milestone);
    assert_eq!(grant.milestone_count, 2);

    // Check milestones stored correctly
    let m0 = t.hub_client.get_milestone(&grant_id, &0);
    assert_eq!(m0.pct, 6000);
    assert_eq!(m0.status, GrantMilestoneStatus::Pending);

    let m1 = t.hub_client.get_milestone(&grant_id, &1);
    assert_eq!(m1.pct, 4000);
    assert_eq!(m1.status, GrantMilestoneStatus::Pending);

    // Submit first milestone
    t.hub_client
        .submit_grant_milestone(&recipient, &grant_id, &0);

    let m0_after = t.hub_client.get_milestone(&grant_id, &0);
    assert_eq!(m0_after.status, GrantMilestoneStatus::Submitted);

    // Approve first milestone
    t.hub_client.approve_grant_milestone(&grant_id, &0);

    // 60% of 10000 = 6000 released
    assert_eq!(t.token.balance(&recipient), 6000);

    let grant_after = t.hub_client.get_grant(&grant_id);
    assert_eq!(grant_after.status, GrantStatus::Executing);

    // Submit and approve second milestone
    t.hub_client
        .submit_grant_milestone(&recipient, &grant_id, &1);
    t.hub_client.approve_grant_milestone(&grant_id, &1);

    // 100% released
    assert_eq!(t.token.balance(&recipient), 10_000);

    let final_grant = t.hub_client.get_grant(&grant_id);
    assert_eq!(final_grant.status, GrantStatus::Completed);
}

#[test]
fn test_retrospective_grant() {
    let t = setup();

    let creator = t.admin.clone();
    StellarAssetClient::new(&t.env, &t.token_addr).mint(&creator, &10_000);

    let applicant1 = Address::generate(&t.env);
    let applicant2 = Address::generate(&t.env);

    let mut options: Vec<String> = Vec::new(&t.env);
    options.push_back(String::from_str(&t.env, "Applicant 1"));
    options.push_back(String::from_str(&t.env, "Applicant 2"));

    let grant_id = t.hub_client.create_retrospective_grant(
        &creator,
        &10_000,
        &t.token_addr,
        &options,
        &604_800, // 1 week
    );

    assert_eq!(grant_id, 1);
    assert_eq!(t.token.balance(&t.escrow_id), 10_000);

    let grant = t.hub_client.get_grant(&grant_id);
    assert_eq!(grant.status, GrantStatus::Active);
    assert_eq!(grant.grant_type, GrantType::Retrospective);

    // Get the session_id to cast votes
    let session_id = t.hub_client.get_retro_session(&grant_id);

    // Cast a vote for option 0 (applicant1)
    let voter = Address::generate(&t.env);
    t.gov_client.cast_vote(&voter, &session_id, &0);

    // Advance time past voting end
    t.env
        .ledger()
        .set_timestamp(t.env.ledger().timestamp() + 700_000);

    // Finalize - applicant1 should get all funds since they got all votes
    let mut recipients: Vec<Address> = Vec::new(&t.env);
    recipients.push_back(applicant1.clone());
    recipients.push_back(applicant2.clone());

    t.hub_client.finalize_retrospective(&grant_id, &recipients);

    let final_grant = t.hub_client.get_grant(&grant_id);
    assert_eq!(final_grant.status, GrantStatus::Completed);

    // applicant1 got all weighted votes, so gets all funds
    assert_eq!(t.token.balance(&applicant1), 10_000);
    assert_eq!(t.token.balance(&applicant2), 0);
}

#[test]
fn test_qf_round() {
    let t = setup();

    let creator = t.admin.clone();
    StellarAssetClient::new(&t.env, &t.token_addr).mint(&creator, &10_000);

    let mut project_names: Vec<String> = Vec::new(&t.env);
    project_names.push_back(String::from_str(&t.env, "Project Alpha"));
    project_names.push_back(String::from_str(&t.env, "Project Beta"));

    let grant_id = t.hub_client.create_qf_round(
        &creator,
        &10_000,
        &t.token_addr,
        &project_names,
        &2_592_000, // 30 days
    );

    assert_eq!(grant_id, 1);
    assert_eq!(t.token.balance(&t.escrow_id), 10_000);

    let grant = t.hub_client.get_grant(&grant_id);
    assert_eq!(grant.status, GrantStatus::Active);
    assert_eq!(grant.grant_type, GrantType::QF);

    let qf_data = t.hub_client.get_qf_round(&grant_id);
    assert_eq!(qf_data.project_count, 2);
    assert_eq!(qf_data.matching_pool, 10_000);

    // Record donations via the hub
    t.hub_client.donate_to_project(&grant_id, &100, &0);
    t.hub_client.donate_to_project(&grant_id, &400, &1);

    // Advance time past session end
    t.env
        .ledger()
        .set_timestamp(t.env.ledger().timestamp() + 3_000_000);

    let p1 = Address::generate(&t.env);
    let p2 = Address::generate(&t.env);
    let mut project_addresses: Vec<Address> = Vec::new(&t.env);
    project_addresses.push_back(p1.clone());
    project_addresses.push_back(p2.clone());

    t.hub_client
        .finalize_qf_round(&grant_id, &project_addresses);

    let final_grant = t.hub_client.get_grant(&grant_id);
    assert_eq!(final_grant.status, GrantStatus::Completed);

    // Verify distribution happened (exact amounts depend on QF math)
    let p1_balance = t.token.balance(&p1);
    let p2_balance = t.token.balance(&p2);

    // Both should have received something
    assert!(p1_balance > 0);
    assert!(p2_balance > 0);
    // Total distributed should equal matching pool
    assert_eq!(p1_balance + p2_balance, 10_000);
}
