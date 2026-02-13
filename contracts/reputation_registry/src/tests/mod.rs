use super::*;
use crate::error::Error;
use crate::storage::ActivityCategory;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

#[test]
fn test_reputation_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    let reg_id = env.register(ReputationRegistry, ());
    let client = ReputationRegistryClient::new(&env, &reg_id);

    let admin = Address::generate(&env);
    client.init_reputation_reg(&admin);

    let user = Address::generate(&env);
    client.init_reputation_reg_profile(&user);

    // Initial state
    let profile = client.get_reputation(&user);
    assert_eq!(profile.overall_score, 0);
    assert_eq!(profile.level, 0);

    // Metadata
    client.set_profile_metadata(&user, &String::from_str(&env, "ipfs://new"));
    assert_eq!(
        client.get_reputation(&user).metadata_cid,
        String::from_str(&env, "ipfs://new")
    );

    // Auth module
    let module_addr = Address::generate(&env);
    client.add_authorized_module(&module_addr);

    // Record completion
    client.record_completion(
        &module_addr,
        &user,
        &101,
        &ActivityCategory::Development,
        &500,
        &false,
        &false,
    );

    let p2 = client.get_reputation(&user);
    assert_eq!(p2.overall_score, 500);
    assert_eq!(p2.bounties_completed, 1);
    // Level = sqrt(500/10) = sqrt(50) = 7
    assert_eq!(p2.level, 7);

    // Unauthorized module
    let bad_module = Address::generate(&env);
    let res = client.try_record_completion(
        &bad_module,
        &user,
        &999,
        &ActivityCategory::Security,
        &100,
        &false,
        &false,
    );
    assert_eq!(res, Err(Ok(Error::ModuleNotAuthorized)));

    // Penalty
    client.record_penalty(&user, &100);
    let p3 = client.get_reputation(&user);
    assert_eq!(p3.overall_score, 400);
    // Level = sqrt(40) = 6
    assert_eq!(p3.level, 6);
}
