use super::*;
use crate::storage::{HackathonStatus, PrizeTier};
use core_escrow::{CoreEscrow, CoreEscrowClient, ModuleType};
use reputation_registry::{ReputationRegistry, ReputationRegistryClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String, Vec,
};

#[test]
fn test_hackathon_v2_full_lifecycle() {
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

    let reg_id = env.register(HackathonRegistry, ());
    let client = HackathonRegistryClient::new(&env, &reg_id);

    // Dummy addresses for mocks
    let proj_reg = Address::generate(&env);
    let voting = Address::generate(&env);

    client.init_hackathon_reg(&admins, &proj_reg, &esc_id, &voting, &rep_id);
    rep_client.add_authorized_module(&reg_id);

    // 2. Create Hackathon Assets
    let organizer = Address::generate(&env);
    let judge1 = Address::generate(&env);
    let judge2 = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let token_admin_client = soroban_sdk::token::StellarAssetClient::new(&env, &asset);

    token_admin_client.mint(&organizer, &100000); // Plenty for main and tracks

    // 3. Create Hackathon
    let main_pool_id = esc_client.create_pool(
        &organizer,
        &ModuleType::Hackathon,
        &1u64,
        &10000i128,
        &asset,
        &(env.ledger().timestamp() + 10000),
        &reg_id,
    );

    let mut tiers = Vec::new(&env);
    tiers.push_back(PrizeTier { rank: 1, pct: 6000 });
    tiers.push_back(PrizeTier { rank: 2, pct: 4000 });

    let mut judges = Vec::new(&env);
    judges.push_back(judge1.clone());
    judges.push_back(judge2.clone());

    let hid = client.create_hackathon(
        &organizer,
        &1u64,
        &String::from_str(&env, "ipfs://meta"),
        &main_pool_id,
        &asset,
        &tiers,
        &1000u64, // submission deadline
        &2000u64, // judging deadline
        &judges,
    );
    assert_eq!(hid, 1);

    // 4. Add Sponsored Track
    let sponsor = Address::generate(&env);
    token_admin_client.mint(&sponsor, &50000);

    let track_pool_id = esc_client.create_pool(
        &sponsor,
        &ModuleType::Hackathon,
        &2u64,
        &5000i128,
        &asset,
        &(env.ledger().timestamp() + 10000),
        &reg_id,
    );

    let tid = client.add_sponsored_track(
        &hid,
        &String::from_str(&env, "Best DeFi"),
        &sponsor,
        &5000i128,
        &track_pool_id,
        &tiers,
    );
    assert_eq!(tid, 1);

    // 5. Submissions
    let lead1 = Address::generate(&env);
    let lead2 = Address::generate(&env);
    rep_client.init_reputation_reg_profile(&lead1);
    rep_client.init_reputation_reg_profile(&lead2);

    let mut tracks = Vec::new(&env);
    tracks.push_back(tid);

    env.ledger().set_timestamp(500);

    client.register_and_submit(
        &lead1,
        &hid,
        &Vec::new(&env),
        &String::from_str(&env, "Project A"),
        &String::from_str(&env, "ipfs://a"),
        &tracks,
    );

    client.register_and_submit(
        &lead2,
        &hid,
        &Vec::new(&env),
        &String::from_str(&env, "Project B"),
        &String::from_str(&env, "ipfs://b"),
        &tracks,
    );

    // 6. Judging
    env.ledger().set_timestamp(1500); // Between submission and judging end

    client.score_submission(&judge1, &hid, &lead1, &80);
    client.score_submission(&judge2, &hid, &lead1, &90);
    client.score_submission(&judge1, &hid, &lead2, &70);
    client.score_submission(&judge2, &hid, &lead2, &70);

    // 7. Finalize
    env.ledger().set_timestamp(2500); // Past judging end
    client.finalize_judging(&hid);

    let sub1 = client.get_submission(&hid, &lead1);
    let sub2 = client.get_submission(&hid, &lead2);

    assert_eq!(sub1.final_score, 8500); // (80+90)/2 * 100
    assert_eq!(sub2.final_score, 7000); // (70+70)/2 * 100

    // 8. Distribution
    let mut rankings = Vec::new(&env);
    rankings.push_back(lead1.clone());
    rankings.push_back(lead2.clone());

    client.distribute_prizes(&hid, &rankings);

    assert_eq!(
        client.get_hackathon(&hid).status,
        HackathonStatus::Completed
    );

    // Check reputation for lead1 (winner)
    let profile = rep_client.get_reputation(&lead1);
    assert!(profile.overall_score >= 1000);
}
