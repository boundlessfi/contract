use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String};

use crate::error::Error;
use crate::events::{ProjectRegistered, ProjectSuspended, VerificationUpgraded, WarningIssued};
use crate::storage::{DataKey, Project};

#[contract]
pub struct ProjectRegistry;

#[contractimpl]
impl ProjectRegistry {
    // ========================================
    // INITIALIZATION
    // ========================================

    pub fn init(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::ProjectCount, &0u64);
        Ok(())
    }

    // ========================================
    // ADMIN: MODULE AUTHORIZATION
    // ========================================

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

    // ========================================
    // PROJECT REGISTRATION
    // ========================================

    pub fn register_project(env: Env, owner: Address, metadata_cid: String) -> Result<u64, Error> {
        owner.require_auth();

        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }

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
            metadata_cid,
            verification_level: 0,
            deposit_held: 0,
            active_bounty_budget: 0,
            bounties_posted: 0,
            total_paid_out: 0,
            avg_rating: 0,
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

        ProjectRegistered { id: count, owner }.publish(&env);

        Ok(count)
    }

    // ========================================
    // VERIFICATION
    // ========================================

    pub fn upgrade_verification(env: Env, project_id: u64, new_level: u32) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        if new_level > 2 {
            return Err(Error::NotAuthorized);
        }

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

    // ========================================
    // BUDGET VALIDATION
    // ========================================

    pub fn validate_budget(env: Env, project_id: u64, budget: i128) -> Result<bool, Error> {
        let project: Project = env
            .storage()
            .persistent()
            .get(&DataKey::Project(project_id))
            .ok_or(Error::ProjectNotFound)?;

        let max_budget: i128 = match project.verification_level {
            0 => 2000,
            1 => 10000,
            _ => return Ok(true), // Level 2+ unlimited
        };

        if budget > max_budget {
            Ok(false)
        } else {
            Ok(true)
        }
    }

    // ========================================
    // AUTHORIZED MODULE FUNCTIONS
    // ========================================

    pub fn record_bounty_posted(
        env: Env,
        module: Address,
        project_id: u64,
        budget: i128,
    ) -> Result<(), Error> {
        Self::require_authorized_module(&env, &module)?;
        module.require_auth();

        let key = DataKey::Project(project_id);
        let mut project: Project = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::ProjectNotFound)?;

        if project.suspended {
            return Err(Error::ProjectSuspended);
        }

        project.bounties_posted += 1;
        project.active_bounty_budget += budget;
        project.total_platform_spend += budget;
        env.storage().persistent().set(&key, &project);

        Ok(())
    }

    pub fn record_payout(
        env: Env,
        module: Address,
        project_id: u64,
        amount: i128,
    ) -> Result<(), Error> {
        Self::require_authorized_module(&env, &module)?;
        module.require_auth();

        let key = DataKey::Project(project_id);
        let mut project: Project = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::ProjectNotFound)?;

        project.total_paid_out += amount;
        if project.active_bounty_budget >= amount {
            project.active_bounty_budget -= amount;
        } else {
            project.active_bounty_budget = 0;
        }
        env.storage().persistent().set(&key, &project);

        Ok(())
    }

    pub fn record_dispute(env: Env, module: Address, project_id: u64) -> Result<(), Error> {
        Self::require_authorized_module(&env, &module)?;
        module.require_auth();

        let key = DataKey::Project(project_id);
        let mut project: Project = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::ProjectNotFound)?;

        project.dispute_count += 1;

        if project.dispute_count >= 3 && project.warning_level < 3 {
            project.warning_level += 1;
            WarningIssued {
                project_id,
                warning_level: project.warning_level,
            }
            .publish(&env);

            if project.warning_level >= 3 {
                project.suspended = true;
                ProjectSuspended { project_id }.publish(&env);
            }
        }

        env.storage().persistent().set(&key, &project);
        Ok(())
    }

    pub fn record_missed_milestone(
        env: Env,
        module: Address,
        project_id: u64,
    ) -> Result<(), Error> {
        Self::require_authorized_module(&env, &module)?;
        module.require_auth();

        let key = DataKey::Project(project_id);
        let mut project: Project = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::ProjectNotFound)?;

        project.missed_milestones += 1;

        if project.missed_milestones >= 3 && project.warning_level < 3 {
            project.warning_level += 1;
            WarningIssued {
                project_id,
                warning_level: project.warning_level,
            }
            .publish(&env);

            if project.warning_level >= 3 {
                project.suspended = true;
                ProjectSuspended { project_id }.publish(&env);
            }
        }

        env.storage().persistent().set(&key, &project);
        Ok(())
    }

    // ========================================
    // ADMIN: SUSPENSION
    // ========================================

    pub fn suspend_project(env: Env, project_id: u64) -> Result<(), Error> {
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

        project.suspended = true;
        env.storage().persistent().set(&key, &project);

        ProjectSuspended { project_id }.publish(&env);
        Ok(())
    }

    pub fn unsuspend_project(env: Env, project_id: u64) -> Result<(), Error> {
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

        project.suspended = false;
        env.storage().persistent().set(&key, &project);

        Ok(())
    }

    // ========================================
    // QUERIES
    // ========================================

    pub fn get_project(env: Env, project_id: u64) -> Result<Project, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Project(project_id))
            .ok_or(Error::ProjectNotFound)
    }

    pub fn is_suspended(env: Env, project_id: u64) -> Result<bool, Error> {
        let project: Project = env
            .storage()
            .persistent()
            .get(&DataKey::Project(project_id))
            .ok_or(Error::ProjectNotFound)?;
        Ok(project.suspended)
    }

    // ========================================
    // UPGRADE
    // ========================================

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    // ========================================
    // INTERNAL HELPERS
    // ========================================

    fn require_authorized_module(env: &Env, caller: &Address) -> Result<(), Error> {
        if !env
            .storage()
            .instance()
            .has(&DataKey::AuthorizedModule(caller.clone()))
        {
            return Err(Error::ModuleNotAuthorized);
        }
        Ok(())
    }
}
