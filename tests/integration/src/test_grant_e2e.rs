/// End-to-end grant tests across CoreEscrow + GovernanceVoting + ReputationRegistry + GrantHub.
/// Tests all 3 grant types: Milestone, Retrospective, Quadratic Funding.
use crate::setup::setup_platform;
use grant_hub::storage::{GrantStatus, GrantType, MilestoneStatus};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, String, Vec};

#[test]
fn test_milestone_grant_full_lifecycle() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);
    let recipient = Address::generate(&p.env);

    p.sac.mint(&creator, &100_000);

    // Create milestones: 60% + 40%
    let mut descs: Vec<(String, u32)> = Vec::new(&p.env);
    descs.push_back((String::from_str(&p.env, "Phase 1"), 6000));
    descs.push_back((String::from_str(&p.env, "Phase 2"), 4000));

    let gid = p
        .grant
        .create_milestone_grant(&creator, &recipient, &20_000, &p.token_addr, &descs);

    // Verify grant created
    let grant = p.grant.get_grant(&gid);
    assert_eq!(grant.status, GrantStatus::Active);
    assert_eq!(grant.grant_type, GrantType::Milestone);
    assert_eq!(grant.milestone_count, 2);

    // Funds moved to escrow
    assert_eq!(p.token.balance(&p.escrow_addr), 20_000);

    // Verify milestones
    let m0 = p.grant.get_milestone(&gid, &0);
    assert_eq!(m0.pct, 6000);
    assert_eq!(m0.status, MilestoneStatus::Pending);

    // Submit and approve milestone 0
    p.grant.submit_grant_milestone(&recipient, &gid, &0);
    assert_eq!(
        p.grant.get_milestone(&gid, &0).status,
        MilestoneStatus::Submitted
    );

    p.grant.approve_grant_milestone(&gid, &0);
    assert_eq!(p.token.balance(&recipient), 12_000); // 60% of 20k

    let grant = p.grant.get_grant(&gid);
    assert_eq!(grant.status, GrantStatus::Executing);

    // Submit and approve milestone 1
    p.grant.submit_grant_milestone(&recipient, &gid, &1);
    p.grant.approve_grant_milestone(&gid, &1);

    assert_eq!(p.token.balance(&recipient), 20_000); // 100% released
    assert_eq!(p.grant.get_grant(&gid).status, GrantStatus::Completed);
}

#[test]
fn test_retrospective_grant_with_voting() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);

    p.sac.mint(&creator, &100_000);

    let applicant1 = Address::generate(&p.env);
    let applicant2 = Address::generate(&p.env);

    let mut options: Vec<String> = Vec::new(&p.env);
    options.push_back(String::from_str(&p.env, "Applicant 1 proposal"));
    options.push_back(String::from_str(&p.env, "Applicant 2 proposal"));

    let gid = p.grant.create_retrospective_grant(
        &creator,
        &10_000,
        &p.token_addr,
        &options,
        &604_800, // 1 week
    );

    assert_eq!(p.grant.get_grant(&gid).status, GrantStatus::Active);
    assert_eq!(p.grant.get_grant(&gid).grant_type, GrantType::Retrospective);
    assert_eq!(p.token.balance(&p.escrow_addr), 10_000);

    // Get the governance session
    let session_id = p.grant.get_retro_session(&gid);

    // Cast votes via GovernanceVoting
    let voter1 = Address::generate(&p.env);
    let voter2 = Address::generate(&p.env);
    let voter3 = Address::generate(&p.env);
    p.governance.cast_vote(&voter1, &session_id, &0); // for applicant1
    p.governance.cast_vote(&voter2, &session_id, &0); // for applicant1
    p.governance.cast_vote(&voter3, &session_id, &1); // for applicant2

    // Advance past voting end
    p.env
        .ledger()
        .set_timestamp(p.env.ledger().timestamp() + 700_000);

    // Finalize retrospective
    let mut recipients: Vec<Address> = Vec::new(&p.env);
    recipients.push_back(applicant1.clone());
    recipients.push_back(applicant2.clone());

    p.grant.finalize_retrospective(&gid, &recipients);

    assert_eq!(p.grant.get_grant(&gid).status, GrantStatus::Completed);

    // applicant1 got 2/3 of votes → gets proportional share
    // applicant2 got 1/3
    // Integer division may lose up to 1 unit (10000 * 1/3 = 3333, 10000 * 2/3 = 6666)
    let bal1 = p.token.balance(&applicant1);
    let bal2 = p.token.balance(&applicant2);
    assert!(bal1 > bal2);
    // Total distributed should be within 1 of the full amount (rounding)
    assert!(bal1 + bal2 >= 9_999);
}

#[test]
fn test_qf_round_with_donations() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);

    p.sac.mint(&creator, &100_000);

    let mut project_names: Vec<String> = Vec::new(&p.env);
    project_names.push_back(String::from_str(&p.env, "Project Alpha"));
    project_names.push_back(String::from_str(&p.env, "Project Beta"));

    let gid = p.grant.create_qf_round(
        &creator,
        &10_000,
        &p.token_addr,
        &project_names,
        &2_592_000, // 30 days
    );

    let grant = p.grant.get_grant(&gid);
    assert_eq!(grant.status, GrantStatus::Active);
    assert_eq!(grant.grant_type, GrantType::QF);

    let qf_data = p.grant.get_qf_round(&gid);
    assert_eq!(qf_data.project_count, 2);
    assert_eq!(qf_data.matching_pool, 10_000);

    // Record donations (simulated amounts via governance QF recording)
    p.grant.donate_to_project(&gid, &100, &0);
    p.grant.donate_to_project(&gid, &400, &1);

    // Advance past session end
    p.env
        .ledger()
        .set_timestamp(p.env.ledger().timestamp() + 3_000_000);

    // Finalize distribution
    let proj1 = Address::generate(&p.env);
    let proj2 = Address::generate(&p.env);
    let mut project_addresses: Vec<Address> = Vec::new(&p.env);
    project_addresses.push_back(proj1.clone());
    project_addresses.push_back(proj2.clone());

    p.grant.finalize_qf_round(&gid, &project_addresses);

    assert_eq!(p.grant.get_grant(&gid).status, GrantStatus::Completed);

    // Both projects received matching funds
    let bal1 = p.token.balance(&proj1);
    let bal2 = p.token.balance(&proj2);
    assert!(bal1 > 0);
    assert!(bal2 > 0);
    assert_eq!(bal1 + bal2, 10_000);
}

#[test]
fn test_milestone_grant_escrow_pool() {
    let p = setup_platform();
    let creator = Address::generate(&p.env);
    let recipient = Address::generate(&p.env);

    p.sac.mint(&creator, &100_000);

    let mut descs: Vec<(String, u32)> = Vec::new(&p.env);
    descs.push_back((String::from_str(&p.env, "All"), 10000));

    let gid = p
        .grant
        .create_milestone_grant(&creator, &recipient, &10_000, &p.token_addr, &descs);

    // Verify full amount deposited to escrow
    assert_eq!(p.token.balance(&p.escrow_addr), 10_000);
    assert_eq!(p.token.balance(&creator), 90_000);

    let grant = p.grant.get_grant(&gid);
    let pool = p.escrow.get_pool(&grant.pool_id);
    assert_eq!(pool.total_deposited, 10_000);
}
