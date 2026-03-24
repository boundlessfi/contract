use super::*;
use crate::storage::VoteContext;
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env, String, Vec};

fn setup_env() -> (Env, GovernanceVotingClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(GovernanceVoting, ());
    let client = GovernanceVotingClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.init(&admin);

    let module = Address::generate(&env);
    client.add_authorized_module(&module);

    (env, client, admin, module)
}

#[test]
fn test_voting_flow() {
    let (env, client, _admin, module) = setup_env();

    let start = env.ledger().timestamp();
    let end = start + 1000;

    let mut options = Vec::new(&env);
    options.push_back(String::from_str(&env, "Yes"));
    options.push_back(String::from_str(&env, "No"));

    let sid = client.create_session(
        &module,
        &VoteContext::CampaignValidation,
        &99,
        &options,
        &start,
        &end,
        &Some(2),
        &None,
        &false,
    );

    let voter1 = Address::generate(&env);
    client.cast_vote(&voter1, &sid, &0);

    let session = client.get_session(&sid);
    assert_eq!(session.total_votes, 1);

    let opt = client.get_option(&sid, &0);
    assert_eq!(opt.votes, 1);
    assert!(!session.threshold_reached);

    let voter2 = Address::generate(&env);
    client.cast_vote(&voter2, &sid, &0);

    let session2 = client.get_session(&sid);
    assert!(session2.threshold_reached);
}

#[test]
fn test_conclude_session() {
    let (env, client, _admin, module) = setup_env();

    let start = env.ledger().timestamp();
    let end = start + 1000;

    let mut options = Vec::new(&env);
    options.push_back(String::from_str(&env, "Yes"));

    let sid = client.create_session(
        &module,
        &VoteContext::CampaignValidation,
        &50,
        &options,
        &start,
        &end,
        &None,
        &None,
        &false,
    );

    // Cannot conclude before end
    let result = client.try_conclude_session(&sid);
    assert!(result.is_err());

    env.ledger().with_mut(|l| {
        l.timestamp = end + 1;
    });

    client.conclude_session(&sid);
    let session = client.get_session(&sid);
    assert_eq!(session.status, crate::storage::VoteStatus::Concluded);
}

#[test]
fn test_double_vote_rejected() {
    let (env, client, _admin, module) = setup_env();

    let start = env.ledger().timestamp();
    let end = start + 1000;

    let mut options = Vec::new(&env);
    options.push_back(String::from_str(&env, "Yes"));
    options.push_back(String::from_str(&env, "No"));

    let sid = client.create_session(
        &module,
        &VoteContext::CampaignValidation,
        &77,
        &options,
        &start,
        &end,
        &None,
        &None,
        &false,
    );

    let voter = Address::generate(&env);
    client.cast_vote(&voter, &sid, &0);

    let result = client.try_cast_vote(&voter, &sid, &1);
    assert!(result.is_err());
}

#[test]
fn test_qf_logic() {
    let (env, client, _admin, module) = setup_env();

    let start = env.ledger().timestamp();
    let end = start + 1000;
    let mut options = Vec::new(&env);
    options.push_back(String::from_str(&env, "P0"));
    options.push_back(String::from_str(&env, "P1"));
    options.push_back(String::from_str(&env, "P2"));

    let sid = client.create_session(
        &module,
        &VoteContext::QFRound,
        &123,
        &options,
        &start,
        &end,
        &None,
        &None,
        &false,
    );

    // Donor A -> Project 1: 100 (sqrt(100*1e6) = 10000)
    // Donor B -> Project 1: 100 (sqrt(100*1e6) = 10000) -> Sum Sqrt = 20000 -> Sq = 400_000_000
    // Donor C -> Project 2: 400 (sqrt(400*1e6) = 20000) -> Sum Sqrt = 20000 -> Sq = 400_000_000
    //
    // Total Sq = 800_000_000
    // Project 1 Share = (400_000_000 / 800_000_000) * 1000 = 500
    // Project 2 Share = (400_000_000 / 800_000_000) * 1000 = 500

    client.record_qf_donation(&sid, &module, &100, &1);
    client.record_qf_donation(&sid, &module, &100, &1);
    client.record_qf_donation(&sid, &module, &400, &2);

    // Jump time to end round
    env.ledger().with_mut(|l| {
        l.timestamp = end + 1;
    });

    let distribution = client.compute_qf_distribution(&sid, &1000);

    let mut p1_share = 0i128;
    let mut p2_share = 0i128;

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

#[test]
fn test_cancel_session() {
    let (env, client, _admin, module) = setup_env();

    let start = env.ledger().timestamp();
    let end = start + 1000;

    let mut options = Vec::new(&env);
    options.push_back(String::from_str(&env, "A"));

    let sid = client.create_session(
        &module,
        &VoteContext::RetrospectiveGrant,
        &42,
        &options,
        &start,
        &end,
        &None,
        &None,
        &false,
    );

    client.cancel_session(&sid);
    let session = client.get_session(&sid);
    assert_eq!(session.status, crate::storage::VoteStatus::Cancelled);
}
