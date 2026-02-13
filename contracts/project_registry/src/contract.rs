use soroban_sdk::{contract, contractimpl, token, Address, Env, IntoVal, String, Symbol, Val, Vec};

use crate::error::Error;
use crate::events::{
    DepositForfeited, DepositLocked, DepositReleased, ProjectRegistered, VerificationUpgraded,
};
use crate::storage::{DataKey, Project};

#[contract]
pub struct ProjectRegistry;

#[contractimpl]
impl ProjectRegistry {
    pub fn init_project_reg(
        env: Env,
        admin: Address,
        token_asset: Address,
        core_escrow: Address,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::TokenAsset, &token_asset);
        env.storage()
            .instance()
            .set(&DataKey::CoreEscrow, &core_escrow);
        env.storage().instance().set(&DataKey::ProjectCount, &0u64);
        Ok(())
    }

    pub fn add_authorized_module(env: Env, module: Address) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.storage()
            .instance()
            .set(&DataKey::AuthorizedModule(module), &true);
        Ok(())
    }

    pub fn remove_authorized_module(env: Env, module: Address) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.storage()
            .instance()
            .remove(&DataKey::AuthorizedModule(module));
        Ok(())
    }

    fn is_authorized(env: &Env, caller: &Address) -> bool {
        env.storage()
            .instance()
            .has(&DataKey::AuthorizedModule(caller.clone()))
    }

    pub fn register_project(
        env: Env,
        owner: Address,
        org_name: String,
        metadata_cid: String,
    ) -> Result<u64, Error> {
        owner.require_auth();

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ProjectCount)
            .unwrap_or(0);
        count += 1;
        env.storage().instance().set(&DataKey::ProjectCount, &count);

        let project = Project {
            id: count,
            owner: owner.clone(),
            org_name,
            metadata_cid,
            verification_level: 0,
            deposit_held: 0,
            active_bounty_budget: 0,
            total_bounties_posted: 0,
            total_paid_out: 0,
            avg_contributor_rating: 0,
            dispute_count: 0,
            missed_milestones: 0,
            warning_level: 0,
            suspended: false,
            hackathons_hosted: 0,
            grants_distributed: 0,
            campaigns_launched: 0,
            total_platform_spend: 0,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Project(count), &project);

        ProjectRegistered {
            project_id: count,
            owner,
        }
        .publish(&env);

        Ok(count)
    }

    pub fn lock_deposit(env: Env, project_id: u64, amount: i128) -> Result<(), Error> {
        let key = DataKey::Project(project_id);
        let mut project: Project = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::ProjectNotFound)?;

        project.owner.require_auth();

        if project.suspended {
            return Err(Error::ProjectSuspended);
        }

        let asset: Address = env
            .storage()
            .instance()
            .get(&DataKey::TokenAsset)
            .ok_or(Error::NotInitialized)?;
        token::Client::new(&env, &asset).transfer(
            &project.owner,
            &env.current_contract_address(),
            &amount,
        );

        project.deposit_held += amount;
        env.storage().persistent().set(&key, &project);

        DepositLocked { project_id, amount }.publish(&env);
        Ok(())
    }

    pub fn release_deposit(
        env: Env,
        caller: Address,
        project_id: u64,
        amount: i128,
    ) -> Result<(), Error> {
        if !Self::is_authorized(&env, &caller) {
            let admin: Address = env
                .storage()
                .instance()
                .get(&DataKey::Admin)
                .ok_or(Error::NotInitialized)?;
            admin.require_auth();
        } else {
            caller.require_auth();
        }

        let key = DataKey::Project(project_id);
        let mut project: Project = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::ProjectNotFound)?;

        if amount > project.deposit_held {
            return Err(Error::InsufficientDeposit);
        }

        let asset: Address = env
            .storage()
            .instance()
            .get(&DataKey::TokenAsset)
            .ok_or(Error::NotInitialized)?;
        token::Client::new(&env, &asset).transfer(
            &env.current_contract_address(),
            &project.owner,
            &amount,
        );

        project.deposit_held -= amount;
        env.storage().persistent().set(&key, &project);

        DepositReleased { project_id, amount }.publish(&env);
        Ok(())
    }

    pub fn forfeit_deposit(env: Env, project_id: u64, amount: i128) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let key = DataKey::Project(project_id);
        let mut project: Project = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::ProjectNotFound)?;

        let asset: Address = env
            .storage()
            .instance()
            .get(&DataKey::TokenAsset)
            .ok_or(Error::NotInitialized)?;
        let core_escrow: Address = env
            .storage()
            .instance()
            .get(&DataKey::CoreEscrow)
            .ok_or(Error::NotInitialized)?;

        token::Client::new(&env, &asset).transfer(
            &env.current_contract_address(),
            &core_escrow,
            &amount,
        );

        let func = Symbol::new(&env, "contribute_insurance");
        let mut args: Vec<Val> = Vec::new(&env);
        args.push_back(amount.into_val(&env));
        args.push_back(asset.into_val(&env));
        env.invoke_contract::<()>(&core_escrow, &func, args);

        project.deposit_held -= amount;
        project.warning_level += 1;
        if project.warning_level >= 3 {
            project.suspended = true;
        }

        env.storage().persistent().set(&key, &project);

        DepositForfeited { project_id, amount }.publish(&env);
        Ok(())
    }

    pub fn upgrade_verification(env: Env, project_id: u64, new_level: u32) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let key = DataKey::Project(project_id);
        let mut project: Project = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::ProjectNotFound)?;
        project.verification_level = new_level;
        env.storage().persistent().set(&key, &project);

        VerificationUpgraded {
            project_id,
            new_level,
        }
        .publish(&env);
        Ok(())
    }

    pub fn set_suspended(env: Env, project_id: u64, suspended: bool) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let key = DataKey::Project(project_id);
        let mut project: Project = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::ProjectNotFound)?;
        project.suspended = suspended;
        env.storage().persistent().set(&key, &project);
        Ok(())
    }

    pub fn update_metadata(env: Env, project_id: u64, metadata_cid: String) -> Result<(), Error> {
        let key = DataKey::Project(project_id);
        let mut project: Project = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::ProjectNotFound)?;
        project.owner.require_auth();
        project.metadata_cid = metadata_cid;
        env.storage().persistent().set(&key, &project);
        Ok(())
    }

    pub fn record_stats(
        env: Env,
        caller: Address,
        project_id: u64,
        bounty_vol: i128,
        grant_vol: i128,
        hackathon_hosted: bool,
    ) -> Result<(), Error> {
        if !Self::is_authorized(&env, &caller) {
            return Err(Error::UnauthorizedCaller);
        }
        caller.require_auth();

        let key = DataKey::Project(project_id);
        let mut project: Project = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::ProjectNotFound)?;

        project.total_paid_out += bounty_vol + grant_vol;
        project.total_platform_spend += bounty_vol + grant_vol;
        if hackathon_hosted {
            project.hackathons_hosted += 1;
        }
        if bounty_vol > 0 {
            project.total_bounties_posted += 1;
        }

        env.storage().persistent().set(&key, &project);
        Ok(())
    }

    pub fn get_project(env: Env, project_id: u64) -> Result<Project, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Project(project_id))
            .ok_or(Error::ProjectNotFound)
    }
}
