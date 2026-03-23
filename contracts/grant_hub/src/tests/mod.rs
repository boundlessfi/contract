use super::*;
use crate::storage::GrantStatus;
use core_escrow::{CoreEscrow, CoreEscrowClient};
use governance_voting::{GovernanceVoting, GovernanceVotingClient};
use payment_router::{ModuleType as RouterModuleType, PaymentRouter, PaymentRouterClient};
use reputation_registry::{ReputationRegistry, ReputationRegistryClient};
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env, String, Vec};

fn setup_env() -> (
    Env,
    GrantHubClient<'static>,
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let esc_id = env.register(CoreEscrow, ());
    let esc_client = CoreEscrowClient::new(&env, &esc_id);
    let fee_account = Address::generate(&env);
    let treasury_escrow = Address::generate(&env);
    esc_client.init_core_escrow(&admin, &fee_account, &treasury_escrow);

    let rep_id = env.register(ReputationRegistry, ());
    let rep_client = ReputationRegistryClient::new(&env, &rep_id);
    rep_client.init_reputation_reg(&admin);

    let gov_id = env.register(GovernanceVoting, ());
    let gov_client = GovernanceVotingClient::new(&env, &gov_id);
    gov_client.init_gov_voting(&admin, &rep_id);

    let router_id = env.register(PaymentRouter, ());
    let router_client = PaymentRouterClient::new(&env, &router_id);
    router_client.init_payment_router(&admin, &admin, &esc_id);
    router_client.set_fee_rate(&RouterModuleType::Grant, &0);

    let hub_id = env.register(GrantHub, ());
    let hub_client = GrantHubClient::new(&env, &hub_id);

    // Authorize GrantHub in ReputationRegistry and GovernanceVoting
    rep_client.add_authorized_module(&hub_id);
    gov_client.add_gov_module(&hub_id);

    let proj_reg = Address::generate(&env);

    hub_client.init_grant_hub(&admin, &proj_reg, &esc_id, &gov_id, &rep_id, &router_id);

    (
        env, hub_client, admin, esc_id, gov_id, rep_id, router_id, proj_reg,
    )
}

#[test]
fn test_milestone_grant_lifecycle() {
    let (env, client, _admin, esc_id, _gov_id, _rep_id, _router_id, _proj_reg) = setup_env();

    let creator = Address::generate(&env);
    let recipient = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let token_admin = soroban_sdk::token::StellarAssetClient::new(&env, &asset);
    token_admin.mint(&creator, &10000);

    let mut milestones = Vec::new(&env);
    milestones.push_back((String::from_str(&env, "M1"), 5000));
    milestones.push_back((String::from_str(&env, "M2"), 5000));

    let gid = client.create_milestone_grant(
        &creator,
        &1,
        &recipient,
        &String::from_str(&env, "ipfs://meta"),
        &10000,
        &asset,
        &milestones,
    );

    assert_eq!(gid, 1);

    let token_client = soroban_sdk::token::Client::new(&env, &asset);
    assert_eq!(token_client.balance(&creator), 0);
    assert_eq!(token_client.balance(&esc_id), 10000);

    let grant_after_creation = client.get_grant(&gid);
    assert_eq!(grant_after_creation.status, GrantStatus::Active);

    // Submission
    client.submit_grant_milestone(&recipient, &gid, &0, &String::from_str(&env, "ipfs://sub"));

    // Approval
    client.approve_grant_milestone(&gid, &0);

    assert_eq!(token_client.balance(&recipient), 5000);

    let grant = client.get_grant(&gid);
    assert_eq!(grant.status, GrantStatus::Active);

    // Second milestone
    client.submit_grant_milestone(&recipient, &gid, &1, &String::from_str(&env, "ipfs://sub2"));
    client.approve_grant_milestone(&gid, &1);

    assert_eq!(token_client.balance(&recipient), 10000);
    let final_grant = client.get_grant(&gid);
    assert_eq!(final_grant.status, GrantStatus::Completed);
}

#[test]
fn test_retrospective_grant_lifecycle() {
    let (env, client, _admin, esc_id, gov_id, _rep_id, _router_id, _proj_reg) = setup_env();

    let creator = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let token_client = soroban_sdk::token::Client::new(&env, &asset);
    soroban_sdk::token::StellarAssetClient::new(&env, &asset).mint(&creator, &10000);
    assert_eq!(token_client.balance(&creator), 10000);

    let applicant1 = Address::generate(&env);
    let applicant2 = Address::generate(&env);
    let mut applicants = Vec::new(&env);
    applicants.push_back(applicant1.clone());
    applicants.push_back(applicant2.clone());

    let gid = client.create_retrospective_grant(
        &creator,
        &1,
        &String::from_str(&env, "ipfs://retro"),
        &10000,
        &asset,
        &applicants,
    );

    assert_eq!(token_client.balance(&esc_id), 10000);

    let grant = client.get_grant(&gid);
    let session_id = grant.vote_session_id.unwrap();

    // Voter
    let voter = Address::generate(&env);
    let rep_client = ReputationRegistryClient::new(&env, &_rep_id);
    rep_client.init_reputation_reg_profile(&voter);

    let gov_client = GovernanceVotingClient::new(&env, &gov_id);
    gov_client.cast_vote(&voter, &session_id, &0); // Vote for applicant1

    // Jump time to end voting
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 700000);

    client.finalize_retrospective(&gid);

    let final_grant = client.get_grant(&gid);
    assert_eq!(final_grant.status, GrantStatus::Completed);
    assert_eq!(final_grant.recipient.unwrap(), applicant1);

    assert_eq!(token_client.balance(&applicant1), 10000);
}

#[test]
fn test_qf_round_lifecycle() {
    let (env, client, _admin, esc_id, gov_id, _rep_id, router_id, _proj_reg) = setup_env();

    let creator = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let token_client = soroban_sdk::token::Client::new(&env, &asset);
    soroban_sdk::token::StellarAssetClient::new(&env, &asset).mint(&creator, &10000);
    assert_eq!(token_client.balance(&creator), 10000);

    let mut eligible = Vec::new(&env);
    eligible.push_back(1u64);
    eligible.push_back(2u64);

    let gid = client.create_qf_round(
        &creator,
        &1,
        &String::from_str(&env, "ipfs://qf"),
        &10000,
        &asset,
        &eligible,
    );

    assert_eq!(token_client.balance(&esc_id), 10000);

    let grant = client.get_grant(&gid);
    let session_id = grant.vote_session_id.unwrap();

    let p1 = Address::generate(&env);
    let p2 = Address::generate(&env);
    let mut project_addresses = Vec::new(&env);
    project_addresses.push_back(p1.clone());
    project_addresses.push_back(p2.clone());

    // Record some donations in Governance
    let gov_client = GovernanceVotingClient::new(&env, &gov_id);
    gov_client.add_gov_module(&router_id); // Authorize router/caller
    let donor = Address::generate(&env);

    gov_client.record_qf_donation(&router_id, &session_id, &donor, &0, &100);
    gov_client.record_qf_donation(&router_id, &session_id, &donor, &1, &400);

    // End round
    env.ledger()
        .set_timestamp(env.ledger().timestamp() + 3000000);

    client.finalize_qf_round(&gid, &project_addresses);

    let final_grant = client.get_grant(&gid);
    assert_eq!(final_grant.status, GrantStatus::Completed);

    assert!(token_client.balance(&p1) >= 2000);
    assert!(token_client.balance(&p2) >= 8000);
}
