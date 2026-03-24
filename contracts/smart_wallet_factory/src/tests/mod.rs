#![cfg(test)]

use crate::contract::{SmartWalletFactory, SmartWalletFactoryClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env};

fn setup_env() -> (Env, SmartWalletFactoryClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let fake_wasm_hash = BytesN::from_array(&env, &[0xAB; 32]);

    let factory_id = env.register(SmartWalletFactory, ());
    let factory_client = SmartWalletFactoryClient::new(&env, &factory_id);

    factory_client.init(&admin, &fake_wasm_hash);

    (env, factory_client, admin)
}

#[test]
fn test_factory_init() {
    let (env, factory_client, _admin) = setup_env();
    assert_eq!(factory_client.get_wallet_count(), 0);
    let hash = factory_client.get_wasm_hash();
    assert_eq!(hash, BytesN::from_array(&env, &[0xAB; 32]));
}

#[test]
fn test_already_initialized() {
    let (env, factory_client, _admin) = setup_env();

    let admin2 = Address::generate(&env);
    let fake_hash = BytesN::from_array(&env, &[0u8; 32]);

    let result = factory_client.try_init(&admin2, &fake_hash);
    assert!(result.is_err());
}

#[test]
fn test_upgrade_template() {
    let (env, factory_client, _admin) = setup_env();

    let new_hash = BytesN::from_array(&env, &[1u8; 32]);
    factory_client.upgrade_template(&new_hash);

    assert_eq!(factory_client.get_wasm_hash(), new_hash);
}

#[test]
fn test_get_wallet_nonexistent() {
    let (_env, factory_client, _admin) = setup_env();

    // Querying a non-existent wallet index should fail
    let result = factory_client.try_get_wallet(&0);
    assert!(result.is_err());
}

#[test]
fn test_get_wallet_by_owner_nonexistent() {
    let (env, factory_client, _admin) = setup_env();

    let mut pk_bytes = [0u8; 65];
    pk_bytes[0] = 0x04;
    let pk = BytesN::from_array(&env, &pk_bytes);

    let result = factory_client.try_get_wallet_by_owner(&pk);
    assert!(result.is_err());
}
