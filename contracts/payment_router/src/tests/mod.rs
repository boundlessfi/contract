use super::*;
use crate::storage::ModuleType;
use core_escrow::{CoreEscrow, CoreEscrowClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_initialize_and_route_deposit() {
    let env = Env::default();
    env.mock_all_auths();

    // Register CoreEscrow
    let core_escrow_id = env.register(CoreEscrow, ());
    let core_escrow_client = CoreEscrowClient::new(&env, &core_escrow_id);
    let escrow_admin = Address::generate(&env);
    core_escrow_client.init_core_escrow(&escrow_admin);

    // Register PaymentRouter
    let router_id = env.register(PaymentRouter, ());
    let router_client = PaymentRouterClient::new(&env, &router_id);

    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    // Initialize Router
    router_client.init_payment_router(&admin, &treasury, &core_escrow_id); // pass Address of contract

    // Setup Token
    let token_admin = Address::generate(&env);
    let asset_address = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let token_client = soroban_sdk::token::Client::new(&env, &asset_address);
    let token_admin_client = soroban_sdk::token::StellarAssetClient::new(&env, &asset_address);

    let payer = Address::generate(&env);
    token_admin_client.mint(&payer, &10000);

    // Test route_deposit
    // Fee for Hackathon is 4% (400 bps)
    // Deposit 1000
    // Fee = 40
    // Insurance = 5% of 40 = 2
    // Treasury = 38
    // Net returned = 960

    let net_amount =
        router_client.route_deposit(&payer, &1000, &asset_address, &ModuleType::Hackathon);

    assert_eq!(net_amount, 960);

    // Check balances
    assert_eq!(token_client.balance(&treasury), 38);
    // Insurance amount should be in CoreEscrow contract
    assert_eq!(token_client.balance(&core_escrow_id), 2);
    // Payer paid 40 (fee only)?
    // Wait, my implementation transfers ONLY `total_fee` from payer!
    // "Transfer to Treasury ... to Insurance"
    // Does it transfer principal? No.
    // So payer balance should be 10000 - 40 = 9960.
    assert_eq!(token_client.balance(&payer), 9960);

    // Check custom fee rate
    router_client.set_fee_rate(&ModuleType::Crowdfund, &1000); // 10%
    let net_amount2 =
        router_client.route_deposit(&payer, &1000, &asset_address, &ModuleType::Crowdfund);
    // Fee = 100
    // Insurance = 5
    // Treasury = 95
    // Net = 900
    assert_eq!(net_amount2, 900);

    assert_eq!(token_client.balance(&treasury), 38 + 95);
    assert_eq!(token_client.balance(&core_escrow_id), 2 + 5);
    assert_eq!(token_client.balance(&payer), 9960 - 100);
}
