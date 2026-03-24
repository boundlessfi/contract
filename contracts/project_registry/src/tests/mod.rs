use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, String};

#[test]
fn test_register_and_query() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(ProjectRegistry, ());
    let client = ProjectRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);

    let owner = Address::generate(&env);
    let pid = client.register_project(&owner, &String::from_str(&env, "ipfs://metadata"));
    assert_eq!(pid, 1);

    let project = client.get_project(&pid);
    assert_eq!(project.id, 1);
    assert_eq!(project.owner, owner);
    assert_eq!(project.verification_level, 0);
    assert_eq!(project.deposit_held, 0);
    assert_eq!(project.active_bounty_budget, 0);
    assert_eq!(project.bounties_posted, 0);
    assert_eq!(project.total_paid_out, 0);
    assert_eq!(project.dispute_count, 0);
    assert_eq!(project.missed_milestones, 0);
    assert_eq!(project.warning_level, 0);
    assert!(!project.suspended);

    // Register a second project
    let owner2 = Address::generate(&env);
    let pid2 = client.register_project(&owner2, &String::from_str(&env, "ipfs://meta2"));
    assert_eq!(pid2, 2);
}

#[test]
fn test_verification_upgrade() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(ProjectRegistry, ());
    let client = ProjectRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);

    let owner = Address::generate(&env);
    let pid = client.register_project(&owner, &String::from_str(&env, "ipfs://meta"));

    // Starts at level 0
    assert_eq!(client.get_project(&pid).verification_level, 0);

    // Upgrade to level 1
    client.upgrade_verification(&pid, &1);
    assert_eq!(client.get_project(&pid).verification_level, 1);

    // Upgrade to level 2
    client.upgrade_verification(&pid, &2);
    assert_eq!(client.get_project(&pid).verification_level, 2);
}

#[test]
fn test_validate_budget() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(ProjectRegistry, ());
    let client = ProjectRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);

    let owner = Address::generate(&env);
    let pid = client.register_project(&owner, &String::from_str(&env, "ipfs://meta"));

    // Level 0: max 2000
    assert!(client.validate_budget(&pid, &2000));
    assert!(!client.validate_budget(&pid, &2001));

    // Upgrade to Level 1: max 10000
    client.upgrade_verification(&pid, &1);
    assert!(client.validate_budget(&pid, &10000));
    assert!(!client.validate_budget(&pid, &10001));

    // Upgrade to Level 2: unlimited
    client.upgrade_verification(&pid, &2);
    assert!(client.validate_budget(&pid, &999_999));
}

#[test]
fn test_warning_escalation() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(ProjectRegistry, ());
    let client = ProjectRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);

    let owner = Address::generate(&env);
    let pid = client.register_project(&owner, &String::from_str(&env, "ipfs://meta"));

    // Add an authorized module
    let module = Address::generate(&env);
    client.add_authorized_module(&module);

    // Record 3 disputes -> warning level increases
    client.record_dispute(&module, &pid);
    assert_eq!(client.get_project(&pid).dispute_count, 1);
    assert_eq!(client.get_project(&pid).warning_level, 0);

    client.record_dispute(&module, &pid);
    assert_eq!(client.get_project(&pid).dispute_count, 2);
    assert_eq!(client.get_project(&pid).warning_level, 0);

    client.record_dispute(&module, &pid);
    assert_eq!(client.get_project(&pid).dispute_count, 3);
    assert_eq!(client.get_project(&pid).warning_level, 1);
}

#[test]
fn test_suspend_project() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(ProjectRegistry, ());
    let client = ProjectRegistryClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);

    let owner = Address::generate(&env);
    let pid = client.register_project(&owner, &String::from_str(&env, "ipfs://meta"));

    assert!(!client.is_suspended(&pid));

    client.suspend_project(&pid);
    assert!(client.is_suspended(&pid));

    let project = client.get_project(&pid);
    assert!(project.suspended);

    // Unsuspend
    client.unsuspend_project(&pid);
    assert!(!client.is_suspended(&pid));
}
