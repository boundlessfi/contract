#![cfg(test)]

use crate::contract::{ReputationRegistry, ReputationRegistryClient};
use boundless_types::ActivityCategory;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

fn setup_env() -> (Env, ReputationRegistryClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(ReputationRegistry, ());
    let client = ReputationRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);

    let module = Address::generate(&env);
    client.add_authorized_module(&module);

    (env, client, admin, module)
}

#[test]
fn test_init_and_profile() {
    let (env, client, _admin, _module) = setup_env();

    let user = Address::generate(&env);
    client.init_profile(&user);

    let profile = client.get_profile(&user);
    assert_eq!(profile.overall_score, 0);
    assert_eq!(profile.level, 0);
    assert_eq!(profile.bounties_completed, 0);
}

#[test]
fn test_record_completion_and_leveling() {
    let (env, client, _admin, module) = setup_env();

    let user = Address::generate(&env);
    client.init_profile(&user);

    // Score 100 points → level = sqrt(100/10) = sqrt(10) = 3
    client.record_completion(&module, &user, &ActivityCategory::Development, &100);

    let profile = client.get_profile(&user);
    assert_eq!(profile.overall_score, 100);
    assert_eq!(profile.level, 3);
    assert_eq!(profile.bounties_completed, 1);
}

#[test]
fn test_record_penalty() {
    let (env, client, _admin, module) = setup_env();

    let user = Address::generate(&env);
    client.init_profile(&user);

    client.record_completion(&module, &user, &ActivityCategory::Security, &50);
    assert_eq!(client.get_profile(&user).overall_score, 50);

    client.record_penalty(&user, &20);
    assert_eq!(client.get_profile(&user).overall_score, 30);

    client.record_penalty(&user, &100);
    assert_eq!(client.get_profile(&user).overall_score, 0);
}

#[test]
fn test_hackathon_result() {
    let (env, client, _admin, module) = setup_env();

    let user = Address::generate(&env);
    client.init_profile(&user);

    client.record_hackathon_result(&module, &user, &30, &true);

    let profile = client.get_profile(&user);
    assert_eq!(profile.hackathons_entered, 1);
    assert_eq!(profile.hackathons_won, 1);
    assert_eq!(profile.overall_score, 30);
}

#[test]
fn test_spark_credits_lifecycle() {
    let (env, client, _admin, module) = setup_env();

    let user = Address::generate(&env);
    client.init_profile(&user);

    assert_eq!(client.get_credits(&user), 3);
    assert!(client.can_apply(&user));

    let result = client.spend_credit(&module, &user);
    assert!(result);
    assert_eq!(client.get_credits(&user), 2);

    client.spend_credit(&module, &user);
    client.spend_credit(&module, &user);
    assert_eq!(client.get_credits(&user), 0);
    assert!(!client.can_apply(&user));

    let result = client.spend_credit(&module, &user);
    assert!(!result);

    client.restore_credit(&module, &user);
    assert_eq!(client.get_credits(&user), 1);

    client.award_credits(&module, &user, &3);
    assert_eq!(client.get_credits(&user), 4);
}

#[test]
fn test_credits_recharge() {
    let (env, client, _admin, module) = setup_env();

    let user = Address::generate(&env);
    client.init_profile(&user);

    client.spend_credit(&module, &user);
    client.spend_credit(&module, &user);
    client.spend_credit(&module, &user);
    assert_eq!(client.get_credits(&user), 0);

    let result = client.try_try_recharge(&user);
    assert!(result.is_err());

    env.ledger().with_mut(|l| {
        l.timestamp += 1_209_601;
    });

    client.try_recharge(&user);
    assert_eq!(client.get_credits(&user), 3);
}

#[test]
fn test_credits_capped_at_max() {
    let (env, client, _admin, module) = setup_env();

    let user = Address::generate(&env);
    client.init_profile(&user);

    client.award_credits(&module, &user, &100);
    assert_eq!(client.get_credits(&user), 10);
}

#[test]
fn test_unauthorized_module_rejected() {
    let (env, client, _admin, _module) = setup_env();

    let unauthorized = Address::generate(&env);
    let user = Address::generate(&env);
    client.init_profile(&user);

    let result = client.try_record_completion(
        &unauthorized,
        &user,
        &ActivityCategory::Development,
        &10,
    );
    assert!(result.is_err());
}
