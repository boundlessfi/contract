use super::*;
use crate::storage::{Campaign, CampaignStatus, DataKey, Milestone, MilestoneStatus};
use core_escrow::{CoreEscrow, CoreEscrowClient};
use payment_router::{PaymentRouter, PaymentRouterClient};
use reputation_registry::{ReputationRegistry, ReputationRegistryClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String, Vec};

#[test]
fn test_crowdfund_full_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();

    // 1. Setup ecosystem
    let admins = Address::generate(&env);
    let fee_account = Address::generate(&env);
    let treasury = Address::generate(&env);

    let esc_id = env.register(CoreEscrow, ());
    let esc_client = CoreEscrowClient::new(&env, &esc_id);
    esc_client.init_core_escrow(&admins, &fee_account, &treasury);

    let rep_id = env.register(ReputationRegistry, ());
    let rep_client = ReputationRegistryClient::new(&env, &rep_id);
    rep_client.init_reputation_reg(&admins);

    let router_id = env.register(PaymentRouter, ());
    let router_client = PaymentRouterClient::new(&env, &router_id);
    router_client.init_payment_router(&admins, &admins, &esc_id);

    let reg_id = env.register(CrowdfundRegistry, ());
    let client = CrowdfundRegistryClient::new(&env, &reg_id);

    // Dummy addresses for mocks
    let proj_reg = Address::generate(&env);
    let voting = Address::generate(&env);

    client.init_crowdfund_reg(&admins, &proj_reg, &esc_id, &voting, &rep_id, &router_id);

    // Assets setup
    let owner = Address::generate(&env);
    let donor1 = Address::generate(&env);
    let donor2 = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let token_admin_client = soroban_sdk::token::StellarAssetClient::new(&env, &asset);

    token_admin_client.mint(&donor1, &10000);
    token_admin_client.mint(&donor2, &10000);

    // 2. Create Campaign
    let mut milestones = Vec::new(&env);
    milestones.push_back(Milestone {
        id: 1,
        description: String::from_str(&env, "MVP"),
        amount: 5000,
        status: MilestoneStatus::Pending,
    });
    milestones.push_back(Milestone {
        id: 2,
        description: String::from_str(&env, "Beta"),
        amount: 5000,
        status: MilestoneStatus::Pending,
    });

    let cid = client.create_campaign(
        &owner,
        &1u64,
        &String::from_str(&env, "ipfs://meta"),
        &10000i128,
        &asset,
        &1000u64, // deadline
        &milestones,
        &1000i128, // min pledge
    );
    assert_eq!(cid, 1);

    // 3. Pledging
    // With 5% fee, we need gross 10527 to reach 10000 net
    client.pledge(&donor1, &cid, &7000);
    client.pledge(&donor2, &cid, &5000);

    let campaign = client.get_campaign(&cid);
    assert!(campaign.current_funding >= 10000);
    assert_eq!(campaign.status, CampaignStatus::Funded);

    // 4. Milestones
    client.submit_milestone(&cid, &1);
    let campaign_after_submit = client.get_campaign(&cid);
    assert_eq!(campaign_after_submit.status, CampaignStatus::Executing);

    client.approve_milestone(&cid, &1);

    let token_client = soroban_sdk::token::Client::new(&env, &asset);
    assert!(token_client.balance(&owner) >= 5000);

    // Release second milestone
    client.submit_milestone(&cid, &2);
    client.approve_milestone(&cid, &2);

    let final_campaign = client.get_campaign(&cid);
    assert_eq!(final_campaign.status, CampaignStatus::Completed);
    assert!(token_client.balance(&owner) >= 10000);
}

#[test]
fn test_refund_fails_if_funded() {
    let env = Env::default();
    env.mock_all_auths();

    let admins = Address::generate(&env);
    let esc_id = env.register(CoreEscrow, ());
    let rep_id = env.register(ReputationRegistry, ());
    let router_id = env.register(PaymentRouter, ());
    let reg_id = env.register(CrowdfundRegistry, ());
    let client = CrowdfundRegistryClient::new(&env, &reg_id);

    client.init_crowdfund_reg(
        &admins,
        &Address::generate(&env),
        &esc_id,
        &Address::generate(&env),
        &rep_id,
        &router_id,
    );

    let owner = Address::generate(&env);
    let asset = Address::generate(&env);
    let cid = client.create_campaign(
        &owner,
        &1,
        &String::from_str(&env, ""),
        &1000,
        &asset,
        &1000,
        &Vec::new(&env),
        &10,
    );

    // Mock successful funding by directly setting storage of the registered contract
    env.as_contract(&reg_id, || {
        let mut campaign: Campaign = env
            .storage()
            .persistent()
            .get(&DataKey::Campaign(cid))
            .unwrap();
        campaign.status = CampaignStatus::Funded;
        env.storage()
            .persistent()
            .set(&DataKey::Campaign(cid), &campaign);
    });

    let donor = Address::generate(&env);

    // When calling via client, errors are returned as Err(Error).
    // The specific error is likely wrapped.
    // For simplicity in this test environment, we expect it to return Err.
    // We can try to match the error if we import it.
    let res = client.try_request_refund(&donor, &cid);
    assert!(res.is_err());
    assert_eq!(
        res.err(),
        Some(Ok(crate::error::Error::CampaignAlreadyFunded))
    );
}
