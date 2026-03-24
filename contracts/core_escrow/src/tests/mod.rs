#![cfg(test)]

use crate::contract::{CoreEscrow, CoreEscrowClient};
use boundless_types::{ModuleType, SubType};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{Address, Env, Vec};

fn setup_env() -> (Env, CoreEscrowClient<'static>, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CoreEscrow, ());
    let client = CoreEscrowClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token_admin = Address::generate(&env);

    let token_addr = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_asset_client = StellarAssetClient::new(&env, &token_addr.address());

    client.init(&admin, &treasury);

    token_asset_client.mint(&admin, &1_000_000);

    (env, client, admin, treasury, token_addr.address())
}

fn mint_to(env: &Env, token: &Address, _admin: &Address, to: &Address, amount: &i128) {
    let sac = StellarAssetClient::new(env, token);
    sac.mint(to, amount);
}

#[test]
fn test_init_and_queries() {
    let (_env, client, admin, treasury, _token) = setup_env();
    assert_eq!(client.get_admin(), admin);
    assert_eq!(client.get_treasury(), treasury);
    assert_eq!(client.get_insurance_balance(), 0);
}

#[test]
fn test_create_pool_and_lock() {
    let (env, client, _admin, _treasury, token) = setup_env();

    let owner = Address::generate(&env);
    let caller = Address::generate(&env);
    mint_to(&env, &token, &_admin, &owner, &100_000);

    let pool_id = client.create_pool(
        &owner,
        &ModuleType::Bounty,
        &1u64,
        &10_000,
        &token,
        &(env.ledger().timestamp() + 86400),
        &caller,
    );

    let pool = client.get_pool(&pool_id);
    assert_eq!(pool.total_deposited, 10_000);
    assert!(!pool.locked);
    assert_eq!(pool.owner, owner);

    client.lock_pool(&pool_id);
    let pool = client.get_pool(&pool_id);
    assert!(pool.locked);
}

#[test]
fn test_define_and_release_slots() {
    let (env, client, _admin, _treasury, token) = setup_env();

    let owner = Address::generate(&env);
    let caller = Address::generate(&env);
    let recipient1 = Address::generate(&env);
    let recipient2 = Address::generate(&env);
    mint_to(&env, &token, &_admin, &owner, &100_000);

    let pool_id = client.create_pool(
        &owner,
        &ModuleType::Bounty,
        &2u64,
        &10_000,
        &token,
        &(env.ledger().timestamp() + 86400),
        &caller,
    );

    let mut slots = Vec::new(&env);
    slots.push_back((recipient1.clone(), 6_000i128));
    slots.push_back((recipient2.clone(), 4_000i128));
    client.define_release_slots(&pool_id, &slots);

    client.release_slot(&pool_id, &0);
    let slot = client.get_slot(&pool_id, &0);
    assert!(slot.released);
    assert_eq!(TokenClient::new(&env, &token).balance(&recipient1), 6_000);

    client.release_slot(&pool_id, &1);
    assert_eq!(TokenClient::new(&env, &token).balance(&recipient2), 4_000);

    let pool = client.get_pool(&pool_id);
    assert_eq!(pool.total_released, 10_000);
}

#[test]
fn test_release_partial() {
    let (env, client, _admin, _treasury, token) = setup_env();

    let owner = Address::generate(&env);
    let caller = Address::generate(&env);
    let recipient = Address::generate(&env);
    mint_to(&env, &token, &_admin, &owner, &100_000);

    let pool_id = client.create_pool(
        &owner,
        &ModuleType::Grant,
        &3u64,
        &50_000,
        &token,
        &(env.ledger().timestamp() + 86400),
        &caller,
    );

    client.release_partial(&pool_id, &recipient, &20_000);
    assert_eq!(TokenClient::new(&env, &token).balance(&recipient), 20_000);

    let pool = client.get_pool(&pool_id);
    assert_eq!(pool.total_released, 20_000);
    assert_eq!(client.get_unreleased(&pool_id), 30_000);
}

#[test]
fn test_refund_all() {
    let (env, client, _admin, _treasury, token) = setup_env();

    let owner = Address::generate(&env);
    let caller = Address::generate(&env);
    mint_to(&env, &token, &_admin, &owner, &100_000);

    let pool_id = client.create_pool(
        &owner,
        &ModuleType::Bounty,
        &4u64,
        &10_000,
        &token,
        &(env.ledger().timestamp() + 86400),
        &caller,
    );

    let balance_before = TokenClient::new(&env, &token).balance(&owner);
    client.refund_all(&pool_id);
    let balance_after = TokenClient::new(&env, &token).balance(&owner);
    assert_eq!(balance_after - balance_before, 10_000);

    let pool = client.get_pool(&pool_id);
    assert_eq!(pool.total_refunded, 10_000);
}

#[test]
fn test_refund_backers() {
    let (env, client, _admin, _treasury, token) = setup_env();

    let owner = Address::generate(&env);
    let caller = Address::generate(&env);
    let backer1 = Address::generate(&env);
    let backer2 = Address::generate(&env);
    mint_to(&env, &token, &_admin, &owner, &100_000);

    let pool_id = client.create_pool(
        &owner,
        &ModuleType::Crowdfund,
        &5u64,
        &10_000,
        &token,
        &(env.ledger().timestamp() + 86400),
        &caller,
    );

    let mut backers = Vec::new(&env);
    backers.push_back((backer1.clone(), 6_000i128));
    backers.push_back((backer2.clone(), 4_000i128));
    client.refund_backers(&pool_id, &backers);

    assert_eq!(TokenClient::new(&env, &token).balance(&backer1), 6_000);
    assert_eq!(TokenClient::new(&env, &token).balance(&backer2), 4_000);
}

#[test]
fn test_route_deposit_with_fees() {
    let (env, client, admin, treasury, token) = setup_env();

    let owner = Address::generate(&env);
    let caller = Address::generate(&env);
    mint_to(&env, &token, &admin, &owner, &100_000);

    let pool_id = client.create_pool(
        &owner,
        &ModuleType::Bounty,
        &10u64,
        &0,
        &token,
        &(env.ledger().timestamp() + 86400),
        &caller,
    );

    let net = client.route_deposit(&owner, &pool_id, &10_000, &token, &SubType::BountyFCFS);

    // 5% fee = 500. Treasury gets 90% of fee = 450. Insurance gets 10% = 50.
    assert_eq!(net, 9_500);
    assert_eq!(TokenClient::new(&env, &token).balance(&treasury), 450);
    assert_eq!(client.get_insurance_balance(), 50);

    let pool = client.get_pool(&pool_id);
    assert_eq!(pool.total_deposited, 9_500);

    let record = client.get_fee_record(&pool_id);
    assert_eq!(record.gross_amount, 10_000);
    assert_eq!(record.fee_amount, 500);
    assert_eq!(record.treasury_cut, 450);
    assert_eq!(record.insurance_cut, 50);
    assert_eq!(record.net_to_escrow, 9_500);
}

#[test]
fn test_route_pledge_fee_on_top() {
    let (env, client, admin, treasury, token) = setup_env();

    let backer = Address::generate(&env);
    let caller = Address::generate(&env);
    let owner = Address::generate(&env);
    mint_to(&env, &token, &admin, &backer, &100_000);
    mint_to(&env, &token, &admin, &owner, &100_000);

    let pool_id = client.create_pool(
        &owner,
        &ModuleType::Crowdfund,
        &20u64,
        &0,
        &token,
        &(env.ledger().timestamp() + 86400),
        &caller,
    );

    let pledged = client.route_pledge(&backer, &pool_id, &1_000, &token);

    assert_eq!(pledged, 1_000);
    let pool = client.get_pool(&pool_id);
    assert_eq!(pool.total_deposited, 1_000);

    // Fee (5% of 1000 = 50) split: treasury 45, insurance 5
    assert_eq!(TokenClient::new(&env, &token).balance(&treasury), 45);
    assert_eq!(client.get_insurance_balance(), 5);

    // Backer paid 1,000 + 50 = 1,050 total
    assert_eq!(
        TokenClient::new(&env, &token).balance(&backer),
        100_000 - 1_050
    );
}

#[test]
fn test_calculate_fee_preview() {
    let (_env, client, _admin, _treasury, _token) = setup_env();

    let (fee, net) = client.calculate_fee(&10_000, &SubType::BountyFCFS);
    assert_eq!(fee, 500);
    assert_eq!(net, 9_500);

    let (fee, net) = client.calculate_fee(&50_000, &SubType::GrantMilestone);
    assert_eq!(fee, 1_500);
    assert_eq!(net, 48_500);

    let (fee, net) = client.calculate_fee(&20_000, &SubType::HackathonMain);
    assert_eq!(fee, 800);
    assert_eq!(net, 19_200);

    let cost = client.calculate_pledge_cost(&1_000);
    assert_eq!(cost, 1_050);
}

#[test]
fn test_set_fee_rate() {
    let (_env, client, _admin, _treasury, _token) = setup_env();

    client.set_fee_rate(&SubType::BountyFCFS, &300);
    assert_eq!(client.get_fee_rate(&SubType::BountyFCFS), 300);

    // Grant rates unchanged
    assert_eq!(client.get_fee_rate(&SubType::GrantMilestone), 300);
    assert_eq!(client.get_fee_rate(&SubType::HackathonMain), 400);
}

#[test]
fn test_pause_resume_routing() {
    let (env, client, admin, _treasury, token) = setup_env();

    let owner = Address::generate(&env);
    let caller = Address::generate(&env);
    mint_to(&env, &token, &admin, &owner, &100_000);

    let pool_id = client.create_pool(
        &owner,
        &ModuleType::Bounty,
        &30u64,
        &0,
        &token,
        &(env.ledger().timestamp() + 86400),
        &caller,
    );

    client.pause_routing();

    let result = client.try_route_deposit(&owner, &pool_id, &10_000, &token, &SubType::BountyFCFS);
    assert!(result.is_err());

    client.resume_routing();

    let net = client.route_deposit(&owner, &pool_id, &10_000, &token, &SubType::BountyFCFS);
    assert_eq!(net, 9_500);
}

#[test]
fn test_insurance_claim() {
    let (env, client, admin, _treasury, token) = setup_env();

    let owner = Address::generate(&env);
    let caller = Address::generate(&env);
    let claimant = Address::generate(&env);
    mint_to(&env, &token, &admin, &owner, &100_000);

    let pool_id = client.create_pool(
        &owner,
        &ModuleType::Bounty,
        &40u64,
        &0,
        &token,
        &(env.ledger().timestamp() + 86400),
        &caller,
    );
    client.route_deposit(&owner, &pool_id, &10_000, &token, &SubType::BountyFCFS);

    assert_eq!(client.get_insurance_balance(), 50);

    client.claim_insurance(&claimant, &30, &token);
    assert_eq!(client.get_insurance_balance(), 20);
    assert_eq!(TokenClient::new(&env, &token).balance(&claimant), 30);
}
