#![cfg(test)]

use crate::contract::{CoreEscrow, CoreEscrowClient};
use crate::storage::ModuleType;
use soroban_sdk::{testutils::Address as _, token, Address, Env};

#[test]
fn test_initialize_and_create_pool() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CoreEscrow, ());
    let client = CoreEscrowClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_account = Address::generate(&env);
    let treasury = Address::generate(&env);
    client.init_core_escrow(&admin, &fee_account, &treasury);

    let owner = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let asset_address = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let module = ModuleType::Hackathon;
    let module_id = 101;
    let total_amount = 1000;
    let expires_at = env.ledger().timestamp() + 1000;

    let token_admin_client = token::StellarAssetClient::new(&env, &asset_address);
    token_admin_client.mint(&owner, &total_amount);

    let pool_id = client.create_pool(
        &owner,
        &module,
        &module_id,
        &total_amount,
        &asset_address,
        &expires_at,
        &owner, // authorized_caller
    );

    client.lock_pool(&pool_id);
}
