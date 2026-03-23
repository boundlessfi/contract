use super::*;
use crate::error::Error;
use core_escrow::{CoreEscrow, CoreEscrowClient};
use soroban_sdk::{testutils::Address as _, token, Address, Env, String};

#[test]
fn test_project_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    // 1. Setup ecosystem dependencies
    let esc_id = env.register(CoreEscrow, ());
    let esc_client = CoreEscrowClient::new(&env, &esc_id);
    let admins = Address::generate(&env);
    let fee_account = Address::generate(&env);
    let treasury = Address::generate(&env);
    esc_client.init_core_escrow(&admins, &fee_account, &treasury);

    let proj_reg_id = env.register(ProjectRegistry, ());
    let client = ProjectRegistryClient::new(&env, &proj_reg_id);

    let admin = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    client.init_project_reg(&admin, &asset, &esc_id);

    // 2. Register Project
    let owner = Address::generate(&env);
    let pid = client.register_project(
        &owner,
        &String::from_str(&env, "Acme Corp"),
        &String::from_str(&env, "ipfs://meta"),
    );
    assert_eq!(pid, 1);

    // 3. Deposit Management
    let token_client = token::Client::new(&env, &asset);
    let token_admin_client = token::StellarAssetClient::new(&env, &asset);
    token_admin_client.mint(&owner, &2000);

    client.lock_deposit(&pid, &500);
    assert_eq!(token_client.balance(&proj_reg_id), 500);

    // 4. Whitelisting & Authorized Calls
    let bounty_module = Address::generate(&env);
    client.add_authorized_module(&bounty_module);

    // Bounty module releases deposit
    client.release_deposit(&bounty_module, &pid, &200);
    assert_eq!(token_client.balance(&owner), 1700); // 1500 (after lock) + 200 (release)

    // Unauthorized call fails (manually check panic or wrap)
    // client.record_stats(&Address::generate(&env), &pid, &100, &0, &false); // Should panic

    // 5. Stats updates
    client.record_stats(&bounty_module, &pid, &500, &0, &false);
    let project = client.get_project(&pid);
    assert_eq!(project.total_paid_out, 500);
    assert_eq!(project.total_bounties_posted, 1);

    // 6. Verification & Suspension
    client.upgrade_verification(&pid, &1); // Level 1
    assert_eq!(client.get_project(&pid).verification_level, 1);

    client.set_suspended(&pid, &true);
    let res = client.try_lock_deposit(&pid, &100);
    assert_eq!(res, Err(Ok(Error::ProjectSuspended)));

    client.set_suspended(&pid, &false);
    client.lock_deposit(&pid, &100); // Works now
    assert_eq!(token_client.balance(&proj_reg_id), 400); // 300 + 100

    // 7. Forfeiture
    client.forfeit_deposit(&pid, &100);
    assert_eq!(token_client.balance(&esc_id), 100);
    assert_eq!(client.get_project(&pid).warning_level, 1);
}

#[test]
fn test_unauthorized_stats_update() {
    let env = Env::default();
    env.mock_all_auths();
    let proj_reg_id = env.register(ProjectRegistry, ());
    let client = ProjectRegistryClient::new(&env, &proj_reg_id);
    let admin = Address::generate(&env);
    let asset = Address::generate(&env);
    let esc = Address::generate(&env);
    client.init_project_reg(&admin, &asset, &esc);

    let owner = Address::generate(&env);
    let pid = client.register_project(
        &owner,
        &String::from_str(&env, "A"),
        &String::from_str(&env, "B"),
    );

    let res = client.try_record_stats(&Address::generate(&env), &pid, &100, &0, &false);
    assert_eq!(res, Err(Ok(Error::UnauthorizedCaller)));
}
