use soroban_sdk::{contract, contractimpl, Address, Env, Map, String};

use crate::error::Error;
use crate::events::{ModuleAuthorized, ScoreUpdated};
use crate::storage::{ActivityCategory, ContributorProfile, DataKey};

#[contract]
pub struct ReputationRegistry;

#[contractimpl]
impl ReputationRegistry {
    pub fn init_reputation_reg(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn init_reputation_reg_profile(env: Env, contributor: Address) -> Result<(), Error> {
        contributor.require_auth();

        let key = DataKey::Profile(contributor.clone());
        if env.storage().persistent().has(&key) {
            return Ok(());
        }

        let profile = ContributorProfile {
            address: contributor.clone(),
            overall_score: 0,
            level: 0,
            category_scores: Map::new(&env),
            bounties_completed: 0,
            hackathons_entered: 0,
            hackathons_won: 0,
            total_earned: 0,
            metadata_cid: String::from_str(&env, ""),
            joined_at: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&key, &profile);
        Ok(())
    }

    pub fn set_profile_metadata(env: Env, contributor: Address, cid: String) -> Result<(), Error> {
        contributor.require_auth();
        let key = DataKey::Profile(contributor.clone());
        let mut profile: ContributorProfile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::ProfileNotFound)?;
        profile.metadata_cid = cid;
        env.storage().persistent().set(&key, &profile);
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
            .set(&DataKey::AuthorizedModule(module.clone()), &true);
        ModuleAuthorized {
            module,
            authorized: true,
        }
        .publish(&env);
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
            .remove(&DataKey::AuthorizedModule(module.clone()));
        ModuleAuthorized {
            module,
            authorized: false,
        }
        .publish(&env);
        Ok(())
    }

    fn is_authorized(env: &Env, module: Address) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::AuthorizedModule(module))
            .unwrap_or(false)
    }

    pub fn record_completion(
        env: Env,
        module: Address,
        contributor: Address,
        _bounty_id: u64,
        category: ActivityCategory,
        points: u32,
        is_hackathon: bool,
        is_win: bool,
    ) -> Result<(), Error> {
        module.require_auth();
        if !Self::is_authorized(&env, module.clone()) {
            return Err(Error::ModuleNotAuthorized);
        }

        let key = DataKey::Profile(contributor.clone());
        let mut profile: ContributorProfile =
            env.storage()
                .persistent()
                .get(&key)
                .unwrap_or(ContributorProfile {
                    address: contributor.clone(),
                    overall_score: 0,
                    level: 0,
                    category_scores: Map::new(&env),
                    bounties_completed: 0,
                    hackathons_entered: 0,
                    hackathons_won: 0,
                    total_earned: 0,
                    metadata_cid: String::from_str(&env, ""),
                    joined_at: env.ledger().timestamp(),
                });

        // Update overall score and category score
        profile.overall_score += points;
        let current_cat_score = profile.category_scores.get(category.clone()).unwrap_or(0);
        profile
            .category_scores
            .set(category, current_cat_score + points);

        if is_hackathon {
            profile.hackathons_entered += 1;
            if is_win {
                profile.hackathons_won += 1;
            }
        } else {
            profile.bounties_completed += 1;
        }

        // Production Leveling: Adaptive square root curve
        // level = sqrt(overall_score / 10)
        profile.level = Self::int_sqrt_u32(profile.overall_score / 10);

        env.storage().persistent().set(&key, &profile);

        ScoreUpdated {
            contributor,
            overall_score: profile.overall_score,
            level: profile.level,
        }
        .publish(&env);

        Ok(())
    }

    pub fn record_penalty(env: Env, contributor: Address, points: u32) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let key = DataKey::Profile(contributor.clone());
        let mut profile: ContributorProfile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::ProfileNotFound)?;

        if profile.overall_score > points {
            profile.overall_score -= points;
        } else {
            profile.overall_score = 0;
        }

        profile.level = Self::int_sqrt_u32(profile.overall_score / 10);
        env.storage().persistent().set(&key, &profile);

        ScoreUpdated {
            contributor,
            overall_score: profile.overall_score,
            level: profile.level,
        }
        .publish(&env);
        Ok(())
    }

    pub fn get_reputation(env: Env, contributor: Address) -> Result<ContributorProfile, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Profile(contributor))
            .ok_or(Error::ProfileNotFound)
    }

    fn int_sqrt_u32(n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let mut x = n;
        let mut y = (x + 1) / 2;
        while y < x {
            x = y;
            y = (x + n / x) / 2;
        }
        x
    }
}
