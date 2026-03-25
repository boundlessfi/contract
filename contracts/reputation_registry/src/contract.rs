use soroban_sdk::{contract, contractimpl, Address, Env, Map, String};

use boundless_types::ttl::{
    INSTANCE_TTL_EXTEND, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD,
};
use boundless_types::ActivityCategory;

use crate::error::ReputationError;
use crate::events::{
    CommunityBonusAdded, CreditsAwarded, CreditsRecharged, CreditsSpent, FraudRecorded,
    ModuleAuthorized, ScoreUpdated,
};
use crate::storage::{ContributorProfile, CreditData, ReputationDataKey};

const RECHARGE_AMOUNT: u32 = 3;
const RECHARGE_INTERVAL: u64 = 1_209_600; // 14 days in seconds
const DEFAULT_MAX_CREDITS: u32 = 10;
const L3_MAX_CREDITS: u32 = 11;

#[contract]
pub struct ReputationRegistry;

#[contractimpl]
impl ReputationRegistry {
    // ========================================================================
    // INITIALIZATION
    // ========================================================================

    pub fn init(env: Env, admin: Address) -> Result<(), ReputationError> {
        if env.storage().instance().has(&ReputationDataKey::Admin) {
            return Err(ReputationError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage()
            .instance()
            .set(&ReputationDataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&ReputationDataKey::Version, &1u32);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    // ========================================================================
    // PROFILE MANAGEMENT
    // ========================================================================

    pub fn init_profile(env: Env, contributor: Address) -> Result<(), ReputationError> {
        contributor.require_auth();

        let key = ReputationDataKey::Profile(contributor.clone());
        if env.storage().persistent().has(&key) {
            return Ok(());
        }

        let now = env.ledger().timestamp();

        let profile = ContributorProfile {
            address: contributor.clone(),
            overall_score: 0,
            level: 0,
            category_scores: Map::new(&env),
            bounties_completed: 0,
            hackathons_entered: 0,
            hackathons_won: 0,
            campaigns_backed: 0,
            grants_received: 0,
            total_earned: 0,
            metadata_cid: String::from_str(&env, ""),
            joined_at: now,
        };

        env.storage().persistent().set(&key, &profile);
        Self::extend_persistent_ttl(&env, &key);

        let credit_key = ReputationDataKey::CreditData(contributor);
        let credits = CreditData::new(now);
        env.storage().persistent().set(&credit_key, &credits);
        Self::extend_persistent_ttl(&env, &credit_key);
        Self::extend_instance_ttl(&env);

        Ok(())
    }

    pub fn set_profile_metadata(
        env: Env,
        contributor: Address,
        cid: String,
    ) -> Result<(), ReputationError> {
        contributor.require_auth();
        let key = ReputationDataKey::Profile(contributor);
        let mut profile: ContributorProfile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ReputationError::ProfileNotFound)?;
        profile.metadata_cid = cid;
        env.storage().persistent().set(&key, &profile);
        Ok(())
    }

    pub fn get_profile(
        env: Env,
        contributor: Address,
    ) -> Result<ContributorProfile, ReputationError> {
        let key = ReputationDataKey::Profile(contributor);
        let profile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ReputationError::ProfileNotFound)?;
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);
        Ok(profile)
    }

    pub fn get_level(env: Env, contributor: Address) -> Result<u32, ReputationError> {
        let profile: ContributorProfile = env
            .storage()
            .persistent()
            .get(&ReputationDataKey::Profile(contributor))
            .ok_or(ReputationError::ProfileNotFound)?;
        Ok(profile.level)
    }

    pub fn meets_requirements(
        env: Env,
        contributor: Address,
        min_level: u32,
    ) -> Result<bool, ReputationError> {
        let profile: ContributorProfile = env
            .storage()
            .persistent()
            .get(&ReputationDataKey::Profile(contributor))
            .ok_or(ReputationError::ProfileNotFound)?;
        Ok(profile.level >= min_level)
    }

    pub fn meets_skill_requirements(
        env: Env,
        contributor: Address,
        min_level: u32,
        required_category: ActivityCategory,
        min_category_score: u32,
    ) -> Result<bool, ReputationError> {
        let profile: ContributorProfile = env
            .storage()
            .persistent()
            .get(&ReputationDataKey::Profile(contributor))
            .ok_or(ReputationError::ProfileNotFound)?;
        if profile.level < min_level {
            return Ok(false);
        }
        let cat_score = profile.category_scores.get(required_category).unwrap_or(0);
        Ok(cat_score >= min_category_score)
    }

    // ========================================================================
    // MODULE AUTHORIZATION
    // ========================================================================

    pub fn add_authorized_module(env: Env, module: Address) -> Result<(), ReputationError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        env.storage()
            .instance()
            .set(&ReputationDataKey::AuthorizedModule(module.clone()), &true);
        ModuleAuthorized {
            module,
            authorized: true,
        }
        .publish(&env);
        Ok(())
    }

    pub fn remove_authorized_module(env: Env, module: Address) -> Result<(), ReputationError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        env.storage()
            .instance()
            .remove(&ReputationDataKey::AuthorizedModule(module.clone()));
        ModuleAuthorized {
            module,
            authorized: false,
        }
        .publish(&env);
        Ok(())
    }

    // ========================================================================
    // REPUTATION RECORDING (called by authorized modules)
    // ========================================================================

    pub fn record_completion(
        env: Env,
        module: Address,
        contributor: Address,
        category: ActivityCategory,
        points: u32,
    ) -> Result<(), ReputationError> {
        module.require_auth();
        Self::require_authorized(&env, &module)?;

        let key = ReputationDataKey::Profile(contributor.clone());
        let mut profile = Self::get_or_create_profile(&env, &contributor);

        profile.overall_score = profile.overall_score.saturating_add(points);
        let current = profile.category_scores.get(category).unwrap_or(0);
        profile
            .category_scores
            .set(category, current.saturating_add(points));
        profile.bounties_completed = profile.bounties_completed.saturating_add(1);

        profile.level = Self::compute_level(profile.overall_score);
        env.storage().persistent().set(&key, &profile);
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);

        ScoreUpdated {
            contributor: contributor.clone(),
            overall_score: profile.overall_score,
            level: profile.level,
        }
        .publish(&env);
        Ok(())
    }

    pub fn record_hackathon_result(
        env: Env,
        module: Address,
        contributor: Address,
        points: u32,
        is_win: bool,
    ) -> Result<(), ReputationError> {
        module.require_auth();
        Self::require_authorized(&env, &module)?;

        let key = ReputationDataKey::Profile(contributor.clone());
        let mut profile = Self::get_or_create_profile(&env, &contributor);

        profile.overall_score = profile.overall_score.saturating_add(points);
        profile.hackathons_entered = profile.hackathons_entered.saturating_add(1);
        if is_win {
            profile.hackathons_won = profile.hackathons_won.saturating_add(1);
        }

        profile.level = Self::compute_level(profile.overall_score);
        env.storage().persistent().set(&key, &profile);

        ScoreUpdated {
            contributor: contributor.clone(),
            overall_score: profile.overall_score,
            level: profile.level,
        }
        .publish(&env);
        Ok(())
    }

    pub fn record_campaign_backed(
        env: Env,
        module: Address,
        backer: Address,
    ) -> Result<(), ReputationError> {
        module.require_auth();
        Self::require_authorized(&env, &module)?;

        let key = ReputationDataKey::Profile(backer.clone());
        let mut profile = Self::get_or_create_profile(&env, &backer);
        profile.campaigns_backed = profile.campaigns_backed.saturating_add(1);
        profile.overall_score = profile.overall_score.saturating_add(5); // small reputation boost
        profile.level = Self::compute_level(profile.overall_score);
        env.storage().persistent().set(&key, &profile);
        Ok(())
    }

    pub fn record_grant_received(
        env: Env,
        module: Address,
        recipient: Address,
        amount: i128,
    ) -> Result<(), ReputationError> {
        module.require_auth();
        Self::require_authorized(&env, &module)?;

        let key = ReputationDataKey::Profile(recipient.clone());
        let mut profile = Self::get_or_create_profile(&env, &recipient);
        profile.grants_received = profile.grants_received.saturating_add(1);
        profile.total_earned = profile.total_earned.saturating_add(amount);
        profile.overall_score = profile.overall_score.saturating_add(20);
        profile.level = Self::compute_level(profile.overall_score);
        env.storage().persistent().set(&key, &profile);
        Ok(())
    }

    pub fn record_penalty(
        env: Env,
        contributor: Address,
        points: u32,
    ) -> Result<(), ReputationError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = ReputationDataKey::Profile(contributor.clone());
        let mut profile: ContributorProfile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ReputationError::ProfileNotFound)?;

        profile.overall_score = profile.overall_score.saturating_sub(points);
        profile.level = Self::compute_level(profile.overall_score);
        env.storage().persistent().set(&key, &profile);

        ScoreUpdated {
            contributor: contributor.clone(),
            overall_score: profile.overall_score,
            level: profile.level,
        }
        .publish(&env);
        Ok(())
    }

    /// Record a contributor abandoning a bounty/task. Called by authorized modules.
    /// Deducts 10 reputation points.
    pub fn record_abandonment(
        env: Env,
        module: Address,
        contributor: Address,
    ) -> Result<(), ReputationError> {
        module.require_auth();
        Self::require_authorized(&env, &module)?;

        let key = ReputationDataKey::Profile(contributor.clone());
        let mut profile: ContributorProfile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ReputationError::ProfileNotFound)?;

        profile.overall_score = profile.overall_score.saturating_sub(10);
        profile.level = Self::compute_level(profile.overall_score);
        env.storage().persistent().set(&key, &profile);

        ScoreUpdated {
            contributor,
            overall_score: profile.overall_score,
            level: profile.level,
        }
        .publish(&env);
        Ok(())
    }

    /// Record a late delivery. Called by authorized modules.
    /// Deducts 5 reputation points.
    pub fn record_late_delivery(
        env: Env,
        module: Address,
        contributor: Address,
    ) -> Result<(), ReputationError> {
        module.require_auth();
        Self::require_authorized(&env, &module)?;

        let key = ReputationDataKey::Profile(contributor.clone());
        let mut profile: ContributorProfile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ReputationError::ProfileNotFound)?;

        profile.overall_score = profile.overall_score.saturating_sub(5);
        profile.level = Self::compute_level(profile.overall_score);
        env.storage().persistent().set(&key, &profile);

        ScoreUpdated {
            contributor,
            overall_score: profile.overall_score,
            level: profile.level,
        }
        .publish(&env);
        Ok(())
    }

    /// Record fraud. Admin-only. Deducts 100 reputation points.
    pub fn record_fraud(env: Env, contributor: Address) -> Result<(), ReputationError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = ReputationDataKey::Profile(contributor.clone());
        let mut profile: ContributorProfile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ReputationError::ProfileNotFound)?;

        profile.overall_score = profile.overall_score.saturating_sub(100);
        profile.level = Self::compute_level(profile.overall_score);
        env.storage().persistent().set(&key, &profile);

        FraudRecorded {
            contributor,
            overall_score: profile.overall_score,
        }
        .publish(&env);
        Ok(())
    }

    /// Add community bonus points. Admin-only.
    pub fn add_community_bonus(
        env: Env,
        contributor: Address,
        reason: String,
        points: u32,
    ) -> Result<(), ReputationError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();

        let key = ReputationDataKey::Profile(contributor.clone());
        let mut profile: ContributorProfile = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ReputationError::ProfileNotFound)?;

        profile.overall_score = profile.overall_score.saturating_add(points);
        profile.level = Self::compute_level(profile.overall_score);
        env.storage().persistent().set(&key, &profile);

        CommunityBonusAdded {
            contributor,
            reason,
            points,
        }
        .publish(&env);
        Ok(())
    }

    // ========================================================================
    // SPARK CREDITS (merged from SparkCredits contract)
    // ========================================================================

    pub fn spend_credit(env: Env, module: Address, user: Address) -> Result<bool, ReputationError> {
        module.require_auth();
        Self::require_authorized(&env, &module)?;

        let key = ReputationDataKey::CreditData(user.clone());
        let mut credits: CreditData = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ReputationError::ProfileNotFound)?;

        if credits.credits == 0 {
            return Ok(false);
        }

        credits.credits -= 1;
        credits.total_spent += 1;
        env.storage().persistent().set(&key, &credits);
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);

        CreditsSpent {
            user,
            remaining: credits.credits,
        }
        .publish(&env);
        Ok(true)
    }

    pub fn restore_credit(env: Env, module: Address, user: Address) -> Result<(), ReputationError> {
        module.require_auth();
        Self::require_authorized(&env, &module)?;

        let key = ReputationDataKey::CreditData(user.clone());
        let mut credits: CreditData = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ReputationError::ProfileNotFound)?;

        if credits.credits < credits.max_credits {
            credits.credits += 1;
            credits.total_earned += 1;
        }
        env.storage().persistent().set(&key, &credits);
        Ok(())
    }

    pub fn award_credits(
        env: Env,
        module: Address,
        user: Address,
        amount: u32,
    ) -> Result<(), ReputationError> {
        module.require_auth();
        Self::require_authorized(&env, &module)?;

        let key = ReputationDataKey::CreditData(user.clone());
        let mut credits: CreditData = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ReputationError::ProfileNotFound)?;

        let new_credits = (credits.credits + amount).min(credits.max_credits);
        let added = new_credits - credits.credits;
        credits.credits = new_credits;
        credits.total_earned += added;
        env.storage().persistent().set(&key, &credits);

        CreditsAwarded {
            user,
            amount: added,
            remaining: credits.credits,
        }
        .publish(&env);
        Ok(())
    }

    /// Permissionless: anyone can trigger recharge for a user after 14 days.
    pub fn try_recharge(env: Env, user: Address) -> Result<(), ReputationError> {
        let key = ReputationDataKey::CreditData(user.clone());
        let mut credits: CreditData = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ReputationError::ProfileNotFound)?;

        let now = env.ledger().timestamp();
        if now < credits.last_recharge + RECHARGE_INTERVAL {
            return Err(ReputationError::RechargeNotReady);
        }

        // Update max for L3+ users
        let profile_key = ReputationDataKey::Profile(user.clone());
        if let Some(profile) = env
            .storage()
            .persistent()
            .get::<_, ContributorProfile>(&profile_key)
        {
            credits.max_credits = if profile.level >= 3 {
                L3_MAX_CREDITS
            } else {
                DEFAULT_MAX_CREDITS
            };
        }

        let new_credits = (credits.credits + RECHARGE_AMOUNT).min(credits.max_credits);
        let added = new_credits - credits.credits;
        credits.credits = new_credits;
        credits.total_earned += added;
        credits.last_recharge = now;
        env.storage().persistent().set(&key, &credits);

        CreditsRecharged {
            user,
            remaining: credits.credits,
        }
        .publish(&env);
        Ok(())
    }

    pub fn get_credits(env: Env, user: Address) -> Result<u32, ReputationError> {
        let credits: CreditData = env
            .storage()
            .persistent()
            .get(&ReputationDataKey::CreditData(user))
            .ok_or(ReputationError::ProfileNotFound)?;
        Ok(credits.credits)
    }

    pub fn can_apply(env: Env, user: Address) -> bool {
        env.storage()
            .persistent()
            .get(&ReputationDataKey::CreditData(user))
            .map(|c: CreditData| c.credits > 0)
            .unwrap_or(false)
    }

    /// Returns the timestamp when the user can next recharge credits.
    pub fn next_recharge_at(env: Env, user: Address) -> Result<u64, ReputationError> {
        let credits: CreditData = env
            .storage()
            .persistent()
            .get(&ReputationDataKey::CreditData(user))
            .ok_or(ReputationError::ProfileNotFound)?;
        Ok(credits.last_recharge + RECHARGE_INTERVAL)
    }

    // ========================================================================
    // ADMIN
    // ========================================================================

    pub fn upgrade(
        env: Env,
        new_wasm_hash: soroban_sdk::BytesN<32>,
    ) -> Result<(), ReputationError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    // ========================================================================
    // INTERNAL HELPERS
    // ========================================================================

    fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND);
    }

    fn extend_persistent_ttl(env: &Env, key: &ReputationDataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND);
    }

    fn require_admin(env: &Env) -> Result<Address, ReputationError> {
        env.storage()
            .instance()
            .get(&ReputationDataKey::Admin)
            .ok_or(ReputationError::NotInitialized)
    }

    fn require_authorized(env: &Env, module: &Address) -> Result<(), ReputationError> {
        let authorized: bool = env
            .storage()
            .instance()
            .get(&ReputationDataKey::AuthorizedModule(module.clone()))
            .unwrap_or(false);
        if !authorized {
            return Err(ReputationError::ModuleNotAuthorized);
        }
        Ok(())
    }

    fn compute_level(score: u32) -> u32 {
        if score == 0 {
            return 0;
        }
        // level = sqrt(score / 10)
        let n = score / 10;
        if n == 0 {
            return 0;
        }
        let mut x = n;
        let mut y = x.div_ceil(2);
        while y < x {
            x = y;
            y = (x + n / x) / 2;
        }
        x
    }

    fn get_or_create_profile(env: &Env, contributor: &Address) -> ContributorProfile {
        let key = ReputationDataKey::Profile(contributor.clone());
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or(ContributorProfile {
                address: contributor.clone(),
                overall_score: 0,
                level: 0,
                category_scores: Map::new(env),
                bounties_completed: 0,
                hackathons_entered: 0,
                hackathons_won: 0,
                campaigns_backed: 0,
                grants_received: 0,
                total_earned: 0,
                metadata_cid: String::from_str(env, ""),
                joined_at: env.ledger().timestamp(),
            })
    }
}
