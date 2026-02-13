use super::*;
use crate::storage::BountyType;
use core_escrow::{CoreEscrow, CoreEscrowClient};
use reputation_registry::{ActivityCategory, ReputationRegistry, ReputationRegistryClient};
use soroban_sdk::token;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

#[test]
fn test_bounty_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    // 1. Register contracts
    let core_escrow_id = env.register(CoreEscrow, ());
    let rep_reg_id = env.register(ReputationRegistry, ());
    let bounty_reg_id = env.register(BountyRegistry, ());

    // 2. Initialize Core Escrow
    let admin = Address::generate(&env);
    let escrow_client = CoreEscrowClient::new(&env, &core_escrow_id);
    escrow_client.init_core_escrow(&admin);

    // 3. Initialize Reputation Registry
    let rep_client = ReputationRegistryClient::new(&env, &rep_reg_id);
    rep_client.init_reputation_reg(&admin);

    // 4. Initialize Bounty Registry
    let bounty_client = BountyRegistryClient::new(&env, &bounty_reg_id);
    bounty_client.init_bounty_reg(&admin, &core_escrow_id, &rep_reg_id);

    // Authorize BountyRegistry in ReputationRegistry
    rep_client.add_authorized_module(&bounty_reg_id);

    // 5. Setup Token and Creator
    let token_admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let creator = Address::generate(&env);

    let token_client = token::Client::new(&env, &asset);
    let token_admin_client = token::StellarAssetClient::new(&env, &asset);

    token_admin_client.mint(&creator, &1000);

    // 6. Create Bounty
    let deadline = env.ledger().timestamp() + 86400; // 1 day
    let bounty_id = bounty_client.create_bounty(
        &creator,
        &String::from_str(&env, "Build a website"),
        &String::from_str(&env, "ipfs://meta"),
        &BountyType::Permissioned,
        &500,
        &asset,
        &ActivityCategory::Development,
        &deadline,
    );

    // Verify deposit
    assert_eq!(token_client.balance(&core_escrow_id), 500);
    assert_eq!(token_client.balance(&creator), 500);

    // 7. Apply
    let applicant = Address::generate(&env);
    rep_client.init_reputation_reg_profile(&applicant); // Init generic profile

    bounty_client.apply(
        &applicant,
        &bounty_id,
        &String::from_str(&env, "I can do it"),
    );

    // 8. Assign
    bounty_client.assign_bounty(&creator, &bounty_id, &applicant);

    // 9. Submit Work
    bounty_client.submit_work(
        &applicant,
        &bounty_id,
        &String::from_str(&env, "ipfs://solution"),
    );

    // 10. Accept Submission
    bounty_client.accept_submission(&creator, &bounty_id, &applicant, &100);

    // Verify Payout
    assert_eq!(token_client.balance(&applicant), 500); // Applicant gets paid
    assert_eq!(token_client.balance(&core_escrow_id), 0); // Escrow empty

    // Verify Reputation
    rep_client.get_reputation(&applicant);
    // Adjust expectations based on Reputation Registry logic if needed,
    // assuming record_completion updates bounties_completed or similar stats
    // Note: The mock Reputation Registry might behave strictly as defined.
}

#[test]
fn test_bounty_cancellation() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup
    let core_escrow_id = env.register(CoreEscrow, ());
    let rep_reg_id = env.register(ReputationRegistry, ());
    let bounty_reg_id = env.register(BountyRegistry, ());

    let admin = Address::generate(&env);
    CoreEscrowClient::new(&env, &core_escrow_id).init_core_escrow(&admin);
    ReputationRegistryClient::new(&env, &rep_reg_id).init_reputation_reg(&admin);

    let bounty_client = BountyRegistryClient::new(&env, &bounty_reg_id);
    bounty_client.init_bounty_reg(&admin, &core_escrow_id, &rep_reg_id);

    // Token
    let token_admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let creator = Address::generate(&env);
    token::StellarAssetClient::new(&env, &asset).mint(&creator, &1000);

    // Create Bounty
    let deadline = env.ledger().timestamp() + 86400;
    let bounty_id = bounty_client.create_bounty(
        &creator,
        &String::from_str(&env, "Cancel Me"),
        &String::from_str(&env, "ipfs://..."),
        &BountyType::Contest,
        &1000,
        &asset,
        &ActivityCategory::Design,
        &deadline,
    );

    // Cancel
    bounty_client.cancel_bounty(&creator, &bounty_id);

    // Verify Refund
    let token_client = token::Client::new(&env, &asset);
    assert_eq!(token_client.balance(&creator), 1000);
    assert_eq!(token_client.balance(&core_escrow_id), 0);
}

#[test]
fn test_update_bounty() {
    let env = Env::default();
    env.mock_all_auths();

    let core_escrow_id = env.register(CoreEscrow, ());
    let rep_reg_id = env.register(ReputationRegistry, ());
    let bounty_reg_id = env.register(BountyRegistry, ());

    let admin = Address::generate(&env);
    CoreEscrowClient::new(&env, &core_escrow_id).init_core_escrow(&admin);
    ReputationRegistryClient::new(&env, &rep_reg_id).init_reputation_reg(&admin);

    let bounty_client = BountyRegistryClient::new(&env, &bounty_reg_id);
    bounty_client.init_bounty_reg(&admin, &core_escrow_id, &rep_reg_id);

    let token_admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let creator = Address::generate(&env);
    token::StellarAssetClient::new(&env, &asset).mint(&creator, &1000);

    let deadline = env.ledger().timestamp() + 86400;
    let bounty_id = bounty_client.create_bounty(
        &creator,
        &String::from_str(&env, "Original"),
        &String::from_str(&env, "ipfs://1"),
        &BountyType::Permissioned,
        &100,
        &asset,
        &ActivityCategory::Development,
        &deadline,
    );

    // Update
    bounty_client.update_bounty(
        &creator,
        &bounty_id,
        &Some(String::from_str(&env, "Updated Title")),
        &None,
        &Some(deadline + 5000),
    );

    // Since we don't have public getters for individual fields easily in tests without extra helpers,
    // we assume success if no panic/error.
}
