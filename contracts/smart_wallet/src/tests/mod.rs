#![cfg(test)]

use crate::contract::{SmartWallet, SmartWalletClient};
use crate::storage::SignerKind;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env};

#[test]
fn test_init_and_query() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SmartWallet, ());
    let client = SmartWalletClient::new(&env, &contract_id);

    // Generate a fake 65-byte public key for testing
    let mut pk_bytes = [0u8; 65];
    pk_bytes[0] = 0x04; // uncompressed point marker
    pk_bytes[1] = 1;
    pk_bytes[33] = 2;
    let owner_pk = BytesN::from_array(&env, &pk_bytes);

    client.init(&owner_pk);

    let stored_pk = client.get_owner_pk();
    assert_eq!(stored_pk, owner_pk);
    assert_eq!(client.get_signer_count(), 0);
}

#[test]
fn test_add_and_remove_signers() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SmartWallet, ());
    let client = SmartWalletClient::new(&env, &contract_id);

    let mut pk_bytes = [0u8; 65];
    pk_bytes[0] = 0x04;
    let owner_pk = BytesN::from_array(&env, &pk_bytes);
    client.init(&owner_pk);

    // Add an address signer
    let signer_addr = Address::generate(&env);
    client.add_signer(&SignerKind::Address(signer_addr.clone()));
    assert_eq!(client.get_signer_count(), 1);

    // Add a secp256r1 signer
    let mut pk2_bytes = [0u8; 65];
    pk2_bytes[0] = 0x04;
    pk2_bytes[1] = 42;
    let signer_pk = BytesN::from_array(&env, &pk2_bytes);
    client.add_signer(&SignerKind::Secp256r1(signer_pk));
    assert_eq!(client.get_signer_count(), 2);

    // Remove the first signer (swap-remove: last moves to slot 0)
    client.remove_signer(&0);
    assert_eq!(client.get_signer_count(), 1);

    // The remaining signer should be the secp256r1 one (it was swapped into slot 0)
    let remaining = client.get_signer(&0);
    match remaining {
        SignerKind::Secp256r1(_) => {} // expected
        _ => panic!("Expected Secp256r1 signer"),
    }
}

#[test]
fn test_duplicate_signer_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SmartWallet, ());
    let client = SmartWalletClient::new(&env, &contract_id);

    let mut pk_bytes = [0u8; 65];
    pk_bytes[0] = 0x04;
    let owner_pk = BytesN::from_array(&env, &pk_bytes);
    client.init(&owner_pk);

    let signer_addr = Address::generate(&env);
    client.add_signer(&SignerKind::Address(signer_addr.clone()));

    // Adding the same signer again should fail
    let result = client.try_add_signer(&SignerKind::Address(signer_addr));
    assert!(result.is_err());
}

#[test]
fn test_already_initialized() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SmartWallet, ());
    let client = SmartWalletClient::new(&env, &contract_id);

    let mut pk_bytes = [0u8; 65];
    pk_bytes[0] = 0x04;
    let owner_pk = BytesN::from_array(&env, &pk_bytes);
    client.init(&owner_pk);

    // Second init should fail
    let result = client.try_init(&owner_pk);
    assert!(result.is_err());
}

#[test]
fn test_ed25519_signer() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(SmartWallet, ());
    let client = SmartWalletClient::new(&env, &contract_id);

    let mut pk_bytes = [0u8; 65];
    pk_bytes[0] = 0x04;
    let owner_pk = BytesN::from_array(&env, &pk_bytes);
    client.init(&owner_pk);

    // Add ed25519 signer
    let ed_pk = BytesN::from_array(&env, &[1u8; 32]);
    client.add_signer(&SignerKind::Ed25519(ed_pk.clone()));
    assert_eq!(client.get_signer_count(), 1);

    let signer = client.get_signer(&0);
    assert_eq!(signer, SignerKind::Ed25519(ed_pk));
}
