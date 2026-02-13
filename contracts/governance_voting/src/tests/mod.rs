use super::*;
use crate::storage::VoteContext;
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env, String, Vec};

#[test]
fn test_voting_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(GovernanceVoting, ());
    let client = GovernanceVotingClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let rep_reg = Address::generate(&env);
    client.init_gov_voting(&admin, &rep_reg);

    let start = env.ledger().timestamp();
    let end = start + 1000;

    let mut options = Vec::new(&env);
    options.push_back(String::from_str(&env, "Yes"));
    options.push_back(String::from_str(&env, "No"));

    let sid = client.create_session(
        &admin,
        &VoteContext::CampaignValidation,
        &99,
        &options,
        &start,
        &end,
        &Some(2),
        &false,
    );

    let voter1 = Address::generate(&env);
    client.cast_vote(&voter1, &sid, &0);

    let session = client.get_session(&sid);
    assert_eq!(session.total_votes, 1);

    let opt = client.get_option(&sid, &0);
    assert_eq!(opt.votes, 1);
    assert!(!session.threshold_reached); // 1 < 2

    let voter2 = Address::generate(&env);
    client.cast_vote(&voter2, &sid, &0);

    let session2 = client.get_session(&sid);
    assert!(session2.threshold_reached); // 2 >= 2
}

#[test]
fn test_qf_logic() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(GovernanceVoting, ());
    let client = GovernanceVotingClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let rep_reg = Address::generate(&env);
    client.init_gov_voting(&admin, &rep_reg);

    // Create session with 3 options (0, 1, 2)
    let start = env.ledger().timestamp();
    let end = start + 1000;
    let mut options = Vec::new(&env);
    options.push_back(String::from_str(&env, "P0")); // index 0
    options.push_back(String::from_str(&env, "P1")); // index 1
    options.push_back(String::from_str(&env, "P2")); // index 2

    let sid = client.create_session(
        &admin,
        &VoteContext::QFRound,
        &123,
        &options,
        &start,
        &end,
        &None,
        &false,
    );

    // Donor A -> Project 1: 100 (sqrt=10)
    // Donor B -> Project 1: 100 (sqrt=10) -> Sum Sqrt = 20 -> Sq = 400
    // Donor C -> Project 2: 400 (sqrt=20) -> Sum Sqrt = 20 -> Sq = 400

    // Total Sq = 800.
    // Matching Pool = 1000.
    // Project 1 Share = (400/800) * 1000 = 500
    // Project 2 Share = (400/800) * 1000 = 500

    let d1 = Address::generate(&env);
    let d2 = Address::generate(&env);
    let d3 = Address::generate(&env);

    let module = Address::generate(&env);
    client.add_gov_module(&module);

    client.record_qf_donation(&module, &sid, &d1, &1, &100);
    client.record_qf_donation(&module, &sid, &d2, &1, &100);
    client.record_qf_donation(&module, &sid, &d3, &2, &400);

    // Jump time to end round
    env.ledger().set_timestamp(end + 1);

    let distribution = client.compute_qf_distribution(&sid, &1000);

    let mut p1_share = 0;
    let mut p2_share = 0;

    for pair in distribution.iter() {
        let (pid, share) = pair;
        if pid == 1 {
            p1_share = share;
        }
        if pid == 2 {
            p2_share = share;
        }
    }

    assert_eq!(p1_share, 500);
    assert_eq!(p2_share, 500);
}
