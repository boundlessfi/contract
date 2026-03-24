use soroban_sdk::{
    contract, contractimpl, Address, BytesN, Env, IntoVal, String, Symbol, Val, Vec,
};

use crate::error::Error;
use crate::events::{HackathonCreated, JudgingFinalized, ProjectSubmitted, TrackAdded};
use crate::storage::{
    DataKey, Hackathon, HackathonStatus, HackathonSubmission, HackathonTrack, PrizeTier,
};
use reputation_registry::ActivityCategory;

#[contract]
pub struct HackathonRegistry;

#[contractimpl]
impl HackathonRegistry {
    pub fn init_hackathon_reg(
        env: Env,
        admin: Address,
        project_registry: Address,
        core_escrow: Address,
        voting_contract: Address,
        reputation_registry: Address,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::ProjectRegistry, &project_registry);
        env.storage()
            .instance()
            .set(&DataKey::CoreEscrow, &core_escrow);
        env.storage()
            .instance()
            .set(&DataKey::VotingContract, &voting_contract);
        env.storage()
            .instance()
            .set(&DataKey::ReputationRegistry, &reputation_registry);
        env.storage()
            .instance()
            .set(&DataKey::HackathonCount, &0u64);
        Ok(())
    }

    pub fn create_hackathon(
        env: Env,
        organizer: Address,
        project_id: u64,
        metadata_cid: String,
        main_pool_id: BytesN<32>,
        asset: Address,
        prize_distribution: Vec<PrizeTier>,
        submission_deadline: u64,
        judging_deadline: u64,
        judges: Vec<Address>,
    ) -> Result<u64, Error> {
        organizer.require_auth();

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::HackathonCount)
            .unwrap_or(0);
        count += 1;
        env.storage()
            .instance()
            .set(&DataKey::HackathonCount, &count);

        let hackathon = Hackathon {
            id: count,
            organizer: organizer.clone(),
            project_id,
            metadata_cid,
            status: HackathonStatus::Published,
            main_pool_id,
            asset,
            judges,
            submission_deadline,
            judging_deadline,
            prize_distribution,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Hackathon(count), &hackathon);
        HackathonCreated {
            id: count,
            organizer,
        }
        .publish(&env);
        Ok(count)
    }

    pub fn add_sponsored_track(
        env: Env,
        hackathon_id: u64,
        name: String,
        sponsor: Address,
        prize_pool: i128,
        pool_id: BytesN<32>,
        prize_distribution: Vec<PrizeTier>,
    ) -> Result<u32, Error> {
        sponsor.require_auth();

        let mut t_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::TrackCount(hackathon_id))
            .unwrap_or(0);
        t_count += 1;
        env.storage()
            .persistent()
            .set(&DataKey::TrackCount(hackathon_id), &t_count);

        let track = HackathonTrack {
            id: t_count,
            name,
            sponsor: sponsor.clone(),
            prize_pool,
            pool_id,
            prize_distribution,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Track(hackathon_id, t_count), &track);
        TrackAdded {
            hackathon_id,
            track_id: t_count,
            sponsor,
        }
        .publish(&env);
        Ok(t_count)
    }

    pub fn register_and_submit(
        env: Env,
        team_lead: Address,
        hackathon_id: u64,
        members: Vec<Address>,
        project_name: String,
        submission_cid: String,
        track_ids: Vec<u32>,
    ) -> Result<(), Error> {
        team_lead.require_auth();

        let h_key = DataKey::Hackathon(hackathon_id);
        let hackathon: Hackathon = env
            .storage()
            .persistent()
            .get(&h_key)
            .ok_or(Error::HackathonNotFound)?;

        if env.ledger().timestamp() > hackathon.submission_deadline {
            return Err(Error::SubmissionClosed);
        }

        let submission = HackathonSubmission {
            team_lead: team_lead.clone(),
            members,
            project_name,
            submission_cid,
            track_ids,
            final_score: 0,
            rank: 0,
        };

        env.storage().persistent().set(
            &DataKey::Submission(hackathon_id, team_lead.clone()),
            &submission,
        );

        let mut leads: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::SubmissionList(hackathon_id))
            .unwrap_or(Vec::new(&env));
        leads.push_back(team_lead.clone());
        env.storage()
            .persistent()
            .set(&DataKey::SubmissionList(hackathon_id), &leads);

        ProjectSubmitted {
            hackathon_id,
            team_lead,
        }
        .publish(&env);
        Ok(())
    }

    pub fn score_submission(
        env: Env,
        judge: Address,
        hackathon_id: u64,
        team_lead: Address,
        score: u32, // 1-100
    ) -> Result<(), Error> {
        judge.require_auth();

        let h_key = DataKey::Hackathon(hackathon_id);
        let hackathon: Hackathon = env
            .storage()
            .persistent()
            .get(&h_key)
            .ok_or(Error::HackathonNotFound)?;

        let mut authorized = false;
        for j in hackathon.judges.iter() {
            if j == judge {
                authorized = true;
                break;
            }
        }
        if !authorized {
            return Err(Error::UnauthorizedJudge);
        }

        if env.ledger().timestamp() < hackathon.submission_deadline
            || env.ledger().timestamp() > hackathon.judging_deadline
        {
            return Err(Error::JudgingNotActive);
        }

        env.storage()
            .persistent()
            .set(&DataKey::JudgeScore(hackathon_id, judge, team_lead), &score);
        Ok(())
    }

    pub fn finalize_judging(env: Env, hackathon_id: u64) -> Result<(), Error> {
        let h_key = DataKey::Hackathon(hackathon_id);
        let mut hackathon: Hackathon = env
            .storage()
            .persistent()
            .get(&h_key)
            .ok_or(Error::HackathonNotFound)?;
        hackathon.organizer.require_auth();

        if env.ledger().timestamp() <= hackathon.judging_deadline {
            return Err(Error::JudgingPeriodNotOver);
        }

        let leads: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::SubmissionList(hackathon_id))
            .ok_or(Error::NoSubmissions)?;

        for lead in leads.iter() {
            let mut total_score: u32 = 0;
            let mut count: u32 = 0;
            for judge in hackathon.judges.iter() {
                if let Some(score) = env
                    .storage()
                    .persistent()
                    .get::<_, u32>(&DataKey::JudgeScore(hackathon_id, judge, lead.clone()))
                {
                    total_score += score;
                    count += 1;
                }
            }

            let mut sub: HackathonSubmission = env
                .storage()
                .persistent()
                .get(&DataKey::Submission(hackathon_id, lead.clone()))
                .unwrap();
            if count > 0 {
                sub.final_score = (total_score * 100) / count;
            }
            env.storage()
                .persistent()
                .set(&DataKey::Submission(hackathon_id, lead), &sub);
        }

        hackathon.status = HackathonStatus::Distributing;
        env.storage().persistent().set(&h_key, &hackathon);

        JudgingFinalized { hackathon_id }.publish(&env);
        Ok(())
    }

    pub fn distribute_prizes(
        env: Env,
        hackathon_id: u64,
        rankings: Vec<Address>, // team_leads in order of rank
    ) -> Result<(), Error> {
        let h_key = DataKey::Hackathon(hackathon_id);
        let mut hackathon: Hackathon = env
            .storage()
            .persistent()
            .get(&h_key)
            .ok_or(Error::HackathonNotFound)?;
        hackathon.organizer.require_auth();

        let esc_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;
        let rep_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::ReputationRegistry)
            .ok_or(Error::NotInitialized)?;

        let mut slots: Vec<(Address, i128)> = Vec::new(&env);

        for tier in hackathon.prize_distribution.iter() {
            if let Some(lead) = rankings.get(tier.rank - 1) {
                let amount: i128 = 1000; // Placeholder
                slots.push_back((lead.clone(), amount));

                let mut rep_args: Vec<Val> = Vec::new(&env);
                rep_args.push_back(env.current_contract_address().into_val(&env));
                rep_args.push_back(lead.into_val(&env));
                rep_args.push_back(0u64.into_val(&env));
                rep_args.push_back(ActivityCategory::Development.into_val(&env));
                rep_args.push_back(1000u32.into_val(&env));
                rep_args.push_back(true.into_val(&env));
                rep_args.push_back(true.into_val(&env));
                env.invoke_contract::<()>(
                    &rep_addr,
                    &Symbol::new(&env, "record_completion"),
                    rep_args,
                );
            }
        }

        let mut esc_args: Vec<Val> = Vec::new(&env);
        esc_args.push_back(hackathon.main_pool_id.clone().into_val(&env));
        esc_args.push_back(slots.into_val(&env));
        env.invoke_contract::<()>(
            &esc_addr,
            &Symbol::new(&env, "define_release_slots"),
            esc_args,
        );

        hackathon.status = HackathonStatus::Completed;
        env.storage().persistent().set(&h_key, &hackathon);
        Ok(())
    }

    pub fn get_hackathon(env: Env, id: u64) -> Result<Hackathon, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Hackathon(id))
            .ok_or(Error::NotFound)
    }

    pub fn get_submission(env: Env, id: u64, lead: Address) -> Result<HackathonSubmission, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Submission(id, lead))
            .ok_or(Error::NotFound)
    }

    // ========================================
    // QUERY FUNCTIONS
    // ========================================

    pub fn get_admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_project_reg(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::ProjectRegistry)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_core_escrow(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_voting_contract(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::VotingContract)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_reputation_reg(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::ReputationRegistry)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_fee_account(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::FeeAccount)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_treasury(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Treasury)
            .ok_or(Error::NotInitialized)
    }

    // ========================================
    // ADMINISTRATIVE FUNCTIONS
    // ========================================

    pub fn update_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        if Self::is_zero_address(&env, &new_admin) {
            panic!("new admin cannot be zero address");
        }

        env.storage().instance().set(&DataKey::Admin, &new_admin);
        Ok(())
    }

    pub fn update_fee_account(env: Env, new_fee_account: Address) -> Result<(), Error> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        if Self::is_zero_address(&env, &new_fee_account) {
            panic!("new fee account cannot be zero address");
        }

        env.storage()
            .instance()
            .set(&DataKey::FeeAccount, &new_fee_account);
        Ok(())
    }

    pub fn update_treasury(env: Env, new_treasury: Address) -> Result<(), Error> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        if Self::is_zero_address(&env, &new_treasury) {
            panic!("new treasury cannot be zero address");
        }

        env.storage()
            .instance()
            .set(&DataKey::Treasury, &new_treasury);
        Ok(())
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    // ========================================
    // INTERNAL HELPERS
    // ========================================

    fn is_zero_address(_env: &Env, _address: &Address) -> bool {
        // Placeholder as Soroban lacks a native zero address.
        false
    }
}
