use crate::error::EscrowError;
use crate::events::{
    FeeCharged, FeeRateUpdated, InsuranceClaimed, InsuranceContributed, PoolCreated, PoolLocked,
    Refunded, SlotReleased,
};
use crate::storage::{EscrowDataKey, EscrowPool, FeeConfig, FeeRecord, InsuranceFund, ReleaseSlot};
use boundless_types::ttl::{
    INSTANCE_TTL_EXTEND, INSTANCE_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND, PERSISTENT_TTL_THRESHOLD,
};
use boundless_types::{math, ModuleType, SubType};
use soroban_sdk::{contract, contractimpl, token, Address, Bytes, BytesN, Env, Vec};

const MAX_FEE_BPS: u32 = 1000;
const MIN_INSURANCE_CUT_BPS: u32 = 500;
const MAX_INSURANCE_CUT_BPS: u32 = 3000;

#[contract]
pub struct CoreEscrow;

#[contractimpl]
impl CoreEscrow {
    // ========================================================================
    // INITIALIZATION
    // ========================================================================

    pub fn init(env: Env, admin: Address, treasury: Address) -> Result<(), EscrowError> {
        if env.storage().instance().has(&EscrowDataKey::Admin) {
            return Err(EscrowError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&EscrowDataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&EscrowDataKey::Treasury, &treasury);
        env.storage()
            .instance()
            .set(&EscrowDataKey::FeeConfig, &FeeConfig::default_config());
        env.storage()
            .instance()
            .set(&EscrowDataKey::RoutingPaused, &false);
        env.storage().instance().set(&EscrowDataKey::Version, &1u32);
        Self::extend_instance_ttl(&env);
        Ok(())
    }

    // ========================================================================
    // QUERIES
    // ========================================================================

    pub fn get_admin(env: Env) -> Result<Address, EscrowError> {
        env.storage()
            .instance()
            .get(&EscrowDataKey::Admin)
            .ok_or(EscrowError::NotInitialized)
    }

    pub fn get_treasury(env: Env) -> Result<Address, EscrowError> {
        env.storage()
            .instance()
            .get(&EscrowDataKey::Treasury)
            .ok_or(EscrowError::NotInitialized)
    }

    pub fn get_pool(env: Env, pool_id: BytesN<32>) -> Result<EscrowPool, EscrowError> {
        let key = EscrowDataKey::EscrowPool(pool_id);
        let pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(EscrowError::PoolNotFound)?;
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);
        Ok(pool)
    }

    pub fn get_slot(env: Env, pool_id: BytesN<32>, index: u32) -> Result<ReleaseSlot, EscrowError> {
        env.storage()
            .persistent()
            .get(&EscrowDataKey::ReleaseSlot(pool_id, index))
            .ok_or(EscrowError::SlotNotFound)
    }

    pub fn get_unreleased(env: Env, pool_id: BytesN<32>) -> Result<i128, EscrowError> {
        let pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&EscrowDataKey::EscrowPool(pool_id))
            .ok_or(EscrowError::PoolNotFound)?;
        pool.total_deposited
            .checked_sub(pool.total_released)
            .ok_or(EscrowError::Overflow)?
            .checked_sub(pool.total_refunded)
            .ok_or(EscrowError::Overflow)
    }

    pub fn is_locked(env: Env, pool_id: BytesN<32>) -> Result<bool, EscrowError> {
        let pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&EscrowDataKey::EscrowPool(pool_id))
            .ok_or(EscrowError::PoolNotFound)?;
        Ok(pool.locked)
    }

    pub fn get_insurance_balance(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&EscrowDataKey::InsuranceFund)
            .map(|f: InsuranceFund| f.balance)
            .unwrap_or(0)
    }

    pub fn get_fee_config(env: Env) -> Result<FeeConfig, EscrowError> {
        env.storage()
            .instance()
            .get(&EscrowDataKey::FeeConfig)
            .ok_or(EscrowError::NotInitialized)
    }

    pub fn get_fee_rate(env: Env, sub_type: SubType) -> Result<u32, EscrowError> {
        let config: FeeConfig = env
            .storage()
            .instance()
            .get(&EscrowDataKey::FeeConfig)
            .ok_or(EscrowError::NotInitialized)?;
        Ok(config.get_fee_bps(&sub_type))
    }

    pub fn calculate_fee(
        env: Env,
        gross: i128,
        sub_type: SubType,
    ) -> Result<(i128, i128), EscrowError> {
        let config: FeeConfig = env
            .storage()
            .instance()
            .get(&EscrowDataKey::FeeConfig)
            .ok_or(EscrowError::NotInitialized)?;
        let fee_bps = config.get_fee_bps(&sub_type);
        let fee = math::calculate_fee_bps(gross, fee_bps).ok_or(EscrowError::Overflow)?;
        let net = gross.checked_sub(fee).ok_or(EscrowError::Overflow)?;
        Ok((fee, net))
    }

    pub fn calculate_pledge_cost(env: Env, pledge: i128) -> Result<i128, EscrowError> {
        let config: FeeConfig = env
            .storage()
            .instance()
            .get(&EscrowDataKey::FeeConfig)
            .ok_or(EscrowError::NotInitialized)?;
        let fee = math::calculate_fee_bps(pledge, config.crowdfund_fee_bps)
            .ok_or(EscrowError::Overflow)?;
        pledge.checked_add(fee).ok_or(EscrowError::Overflow)
    }

    pub fn get_fee_record(env: Env, pool_id: BytesN<32>) -> Result<FeeRecord, EscrowError> {
        env.storage()
            .persistent()
            .get(&EscrowDataKey::FeeRecord(pool_id))
            .ok_or(EscrowError::PoolNotFound)
    }

    // ========================================================================
    // ADMIN FUNCTIONS
    // ========================================================================

    pub fn update_admin(env: Env, new_admin: Address) -> Result<(), EscrowError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        env.storage()
            .instance()
            .set(&EscrowDataKey::Admin, &new_admin);
        Ok(())
    }

    pub fn update_treasury(env: Env, new_treasury: Address) -> Result<(), EscrowError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        env.storage()
            .instance()
            .set(&EscrowDataKey::Treasury, &new_treasury);
        Ok(())
    }

    pub fn set_fee_rate(env: Env, sub_type: SubType, new_bps: u32) -> Result<(), EscrowError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        if new_bps > MAX_FEE_BPS {
            return Err(EscrowError::RateExceedsLimit);
        }
        let mut config: FeeConfig = env
            .storage()
            .instance()
            .get(&EscrowDataKey::FeeConfig)
            .ok_or(EscrowError::NotInitialized)?;
        let old_bps = config.get_fee_bps(&sub_type);
        match sub_type {
            SubType::BountyFCFS
            | SubType::BountyApplication
            | SubType::BountyContest
            | SubType::BountySplit => config.bounty_fee_bps = new_bps,
            SubType::CrowdfundPledge => config.crowdfund_fee_bps = new_bps,
            SubType::GrantMilestone
            | SubType::GrantRetrospective
            | SubType::GrantQFMatchingPool => config.grant_fee_bps = new_bps,
            SubType::HackathonMain | SubType::HackathonTrack => config.hackathon_fee_bps = new_bps,
        }
        env.storage()
            .instance()
            .set(&EscrowDataKey::FeeConfig, &config);
        FeeRateUpdated { old_bps, new_bps }.publish(&env);
        Ok(())
    }

    pub fn set_insurance_cut(env: Env, new_bps: u32) -> Result<(), EscrowError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        if !(MIN_INSURANCE_CUT_BPS..=MAX_INSURANCE_CUT_BPS).contains(&new_bps) {
            return Err(EscrowError::InsuranceCutOutOfRange);
        }
        let mut config: FeeConfig = env
            .storage()
            .instance()
            .get(&EscrowDataKey::FeeConfig)
            .ok_or(EscrowError::NotInitialized)?;
        config.insurance_cut_bps = new_bps;
        env.storage()
            .instance()
            .set(&EscrowDataKey::FeeConfig, &config);
        Ok(())
    }

    pub fn pause_routing(env: Env) -> Result<(), EscrowError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        env.storage()
            .instance()
            .set(&EscrowDataKey::RoutingPaused, &true);
        Ok(())
    }

    pub fn resume_routing(env: Env) -> Result<(), EscrowError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        env.storage()
            .instance()
            .set(&EscrowDataKey::RoutingPaused, &false);
        Ok(())
    }

    pub fn authorize_module(env: Env, module_addr: Address) -> Result<(), EscrowError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        env.storage()
            .instance()
            .set(&EscrowDataKey::AuthorizedModule(module_addr), &true);
        Ok(())
    }

    pub fn deauthorize_module(env: Env, module_addr: Address) -> Result<(), EscrowError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        env.storage()
            .instance()
            .remove(&EscrowDataKey::AuthorizedModule(module_addr));
        Ok(())
    }

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), EscrowError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }

    // ========================================================================
    // POOL MANAGEMENT
    // ========================================================================

    pub fn create_pool(
        env: Env,
        owner: Address,
        module: ModuleType,
        module_id: u64,
        total_amount: i128,
        asset: Address,
        expires_at: u64,
        authorized_caller: Address,
    ) -> Result<BytesN<32>, EscrowError> {
        owner.require_auth();
        if total_amount < 0 {
            return Err(EscrowError::InvalidAmount);
        }
        let pool_id = Self::compute_pool_id(&env, &module, module_id);
        if env
            .storage()
            .persistent()
            .has(&EscrowDataKey::EscrowPool(pool_id.clone()))
        {
            return Err(EscrowError::PoolAlreadyExists);
        }
        if total_amount > 0 {
            token::Client::new(&env, &asset).transfer(
                &owner,
                &env.current_contract_address(),
                &total_amount,
            );
        }
        let pool = EscrowPool {
            pool_id: pool_id.clone(),
            module,
            authorized_caller,
            owner: owner.clone(),
            total_deposited: total_amount,
            total_released: 0,
            total_refunded: 0,
            asset,
            locked: false,
            created_at: env.ledger().timestamp(),
            expires_at,
        };
        let key = EscrowDataKey::EscrowPool(pool_id.clone());
        env.storage().persistent().set(&key, &pool);
        Self::extend_persistent_ttl(&env, &key);
        Self::extend_instance_ttl(&env);
        PoolCreated {
            pool_id: pool_id.clone(),
            owner,
            module,
            total_amount,
        }
        .publish(&env);
        Ok(pool_id)
    }

    pub fn deposit(
        env: Env,
        pool_id: BytesN<32>,
        amount: i128,
        payer: Address,
    ) -> Result<(), EscrowError> {
        payer.require_auth();
        if amount <= 0 {
            return Err(EscrowError::InvalidAmount);
        }
        let key = EscrowDataKey::EscrowPool(pool_id);
        let mut pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(EscrowError::PoolNotFound)?;
        if pool.locked {
            return Err(EscrowError::PoolLocked);
        }
        token::Client::new(&env, &pool.asset).transfer(
            &payer,
            &env.current_contract_address(),
            &amount,
        );
        pool.total_deposited = pool
            .total_deposited
            .checked_add(amount)
            .ok_or(EscrowError::Overflow)?;
        env.storage().persistent().set(&key, &pool);
        Ok(())
    }

    pub fn lock_pool(env: Env, pool_id: BytesN<32>) -> Result<(), EscrowError> {
        let key = EscrowDataKey::EscrowPool(pool_id.clone());
        let mut pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(EscrowError::PoolNotFound)?;
        pool.authorized_caller.require_auth();
        if pool.locked {
            return Err(EscrowError::PoolLocked);
        }
        pool.locked = true;
        env.storage().persistent().set(&key, &pool);
        PoolLocked {
            pool_id: pool_id.clone(),
        }
        .publish(&env);
        Ok(())
    }

    pub fn define_release_slots(
        env: Env,
        pool_id: BytesN<32>,
        slots: Vec<(Address, i128)>,
    ) -> Result<(), EscrowError> {
        let key = EscrowDataKey::EscrowPool(pool_id.clone());
        let pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(EscrowError::PoolNotFound)?;
        pool.authorized_caller.require_auth();
        let mut total_slot_amount: i128 = 0;
        for (i, (recipient, amount)) in slots.iter().enumerate() {
            let slot = ReleaseSlot {
                pool_id: pool_id.clone(),
                slot_index: i as u32,
                amount,
                recipient: recipient.clone(),
                released: false,
                released_at: None,
            };
            env.storage().persistent().set(
                &EscrowDataKey::ReleaseSlot(pool_id.clone(), i as u32),
                &slot,
            );
            total_slot_amount = total_slot_amount
                .checked_add(amount)
                .ok_or(EscrowError::Overflow)?;
        }
        if total_slot_amount > pool.total_deposited {
            return Err(EscrowError::SlotsExceedDeposit);
        }
        env.storage()
            .persistent()
            .set(&EscrowDataKey::SlotCount(pool_id), &slots.len());
        Ok(())
    }

    // ========================================================================
    // RELEASE FUNCTIONS
    // ========================================================================

    pub fn release_slot(env: Env, pool_id: BytesN<32>, slot_index: u32) -> Result<(), EscrowError> {
        let pool_key = EscrowDataKey::EscrowPool(pool_id.clone());
        let mut pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&pool_key)
            .ok_or(EscrowError::PoolNotFound)?;
        pool.authorized_caller.require_auth();
        let slot_key = EscrowDataKey::ReleaseSlot(pool_id.clone(), slot_index);
        let mut slot: ReleaseSlot = env
            .storage()
            .persistent()
            .get(&slot_key)
            .ok_or(EscrowError::SlotNotFound)?;
        if slot.released {
            return Err(EscrowError::SlotAlreadyReleased);
        }
        slot.released = true;
        slot.released_at = Some(env.ledger().timestamp());
        pool.total_released = pool
            .total_released
            .checked_add(slot.amount)
            .ok_or(EscrowError::Overflow)?;
        env.storage().persistent().set(&pool_key, &pool);
        env.storage().persistent().set(&slot_key, &slot);
        token::Client::new(&env, &pool.asset).transfer(
            &env.current_contract_address(),
            &slot.recipient,
            &slot.amount,
        );
        SlotReleased {
            pool_id,
            slot_index,
            recipient: slot.recipient,
            amount: slot.amount,
        }
        .publish(&env);
        Ok(())
    }

    pub fn release_partial(
        env: Env,
        pool_id: BytesN<32>,
        recipient: Address,
        amount: i128,
    ) -> Result<(), EscrowError> {
        if amount <= 0 {
            return Err(EscrowError::InvalidAmount);
        }
        let pool_key = EscrowDataKey::EscrowPool(pool_id.clone());
        let mut pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&pool_key)
            .ok_or(EscrowError::PoolNotFound)?;
        pool.authorized_caller.require_auth();
        let remaining = pool
            .total_deposited
            .checked_sub(pool.total_released)
            .ok_or(EscrowError::Overflow)?
            .checked_sub(pool.total_refunded)
            .ok_or(EscrowError::Overflow)?;
        if amount > remaining {
            return Err(EscrowError::InsufficientFunds);
        }
        pool.total_released = pool
            .total_released
            .checked_add(amount)
            .ok_or(EscrowError::Overflow)?;
        env.storage().persistent().set(&pool_key, &pool);
        token::Client::new(&env, &pool.asset).transfer(
            &env.current_contract_address(),
            &recipient,
            &amount,
        );
        SlotReleased {
            pool_id,
            slot_index: u32::MAX,
            recipient,
            amount,
        }
        .publish(&env);
        Ok(())
    }

    // ========================================================================
    // REFUND FUNCTIONS
    // ========================================================================

    pub fn refund_all(env: Env, pool_id: BytesN<32>) -> Result<(), EscrowError> {
        let pool_key = EscrowDataKey::EscrowPool(pool_id.clone());
        let mut pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&pool_key)
            .ok_or(EscrowError::PoolNotFound)?;
        pool.authorized_caller.require_auth();
        let remaining = pool
            .total_deposited
            .checked_sub(pool.total_released)
            .ok_or(EscrowError::Overflow)?
            .checked_sub(pool.total_refunded)
            .ok_or(EscrowError::Overflow)?;
        if remaining > 0 {
            pool.total_refunded = pool
                .total_refunded
                .checked_add(remaining)
                .ok_or(EscrowError::Overflow)?;
            env.storage().persistent().set(&pool_key, &pool);
            token::Client::new(&env, &pool.asset).transfer(
                &env.current_contract_address(),
                &pool.owner,
                &remaining,
            );
            Refunded {
                pool_id,
                recipient: pool.owner,
                amount: remaining,
            }
            .publish(&env);
        }
        Ok(())
    }

    pub fn refund_remaining(env: Env, pool_id: BytesN<32>) -> Result<(), EscrowError> {
        Self::refund_all(env, pool_id)
    }

    pub fn refund_backers(
        env: Env,
        pool_id: BytesN<32>,
        backers: Vec<(Address, i128)>,
    ) -> Result<(), EscrowError> {
        let pool_key = EscrowDataKey::EscrowPool(pool_id.clone());
        let mut pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&pool_key)
            .ok_or(EscrowError::PoolNotFound)?;
        pool.authorized_caller.require_auth();
        let token_client = token::Client::new(&env, &pool.asset);
        let contract_addr = env.current_contract_address();
        for (backer, amount) in backers.iter() {
            if amount > 0 {
                pool.total_refunded = pool
                    .total_refunded
                    .checked_add(amount)
                    .ok_or(EscrowError::Overflow)?;
                token_client.transfer(&contract_addr, &backer, &amount);
                Refunded {
                    pool_id: pool_id.clone(),
                    recipient: backer,
                    amount,
                }
                .publish(&env);
            }
        }
        env.storage().persistent().set(&pool_key, &pool);
        Ok(())
    }

    // ========================================================================
    // INSURANCE FUND
    // ========================================================================

    pub fn contribute_insurance(env: Env, amount: i128) -> Result<(), EscrowError> {
        if amount <= 0 {
            return Err(EscrowError::InvalidAmount);
        }
        let mut fund = env
            .storage()
            .instance()
            .get(&EscrowDataKey::InsuranceFund)
            .unwrap_or(InsuranceFund {
                balance: 0,
                total_contributions: 0,
                total_paid_out: 0,
            });
        fund.balance = fund
            .balance
            .checked_add(amount)
            .ok_or(EscrowError::Overflow)?;
        fund.total_contributions = fund
            .total_contributions
            .checked_add(amount)
            .ok_or(EscrowError::Overflow)?;
        env.storage()
            .instance()
            .set(&EscrowDataKey::InsuranceFund, &fund);
        InsuranceContributed { amount }.publish(&env);
        Ok(())
    }

    pub fn claim_insurance(
        env: Env,
        claimant: Address,
        amount: i128,
        asset: Address,
    ) -> Result<(), EscrowError> {
        let admin = Self::require_admin(&env)?;
        admin.require_auth();
        let mut fund: InsuranceFund = env
            .storage()
            .instance()
            .get(&EscrowDataKey::InsuranceFund)
            .ok_or(EscrowError::NotInitialized)?;
        if amount > fund.balance {
            return Err(EscrowError::InsuranceInsufficient);
        }
        fund.balance = fund
            .balance
            .checked_sub(amount)
            .ok_or(EscrowError::Overflow)?;
        fund.total_paid_out = fund
            .total_paid_out
            .checked_add(amount)
            .ok_or(EscrowError::Overflow)?;
        env.storage()
            .instance()
            .set(&EscrowDataKey::InsuranceFund, &fund);
        token::Client::new(&env, &asset).transfer(
            &env.current_contract_address(),
            &claimant,
            &amount,
        );
        InsuranceClaimed { claimant, amount }.publish(&env);
        Ok(())
    }

    // ========================================================================
    // FEE ROUTING (merged from PaymentRouter)
    // ========================================================================

    pub fn route_deposit(
        env: Env,
        payer: Address,
        pool_id: BytesN<32>,
        gross_amount: i128,
        asset: Address,
        sub_type: SubType,
    ) -> Result<i128, EscrowError> {
        payer.require_auth();
        Self::require_not_paused(&env)?;
        if gross_amount <= 0 {
            return Err(EscrowError::InvalidAmount);
        }
        let config: FeeConfig = env
            .storage()
            .instance()
            .get(&EscrowDataKey::FeeConfig)
            .ok_or(EscrowError::NotInitialized)?;
        let treasury: Address = env
            .storage()
            .instance()
            .get(&EscrowDataKey::Treasury)
            .ok_or(EscrowError::NotInitialized)?;
        let fee_bps = config.get_fee_bps(&sub_type);
        let fee = math::calculate_fee_bps(gross_amount, fee_bps).ok_or(EscrowError::Overflow)?;
        let (treasury_cut, insurance_cut) =
            math::split_fee(fee, config.insurance_cut_bps).ok_or(EscrowError::Overflow)?;
        let net = gross_amount.checked_sub(fee).ok_or(EscrowError::Overflow)?;
        let token_client = token::Client::new(&env, &asset);
        let contract_addr = env.current_contract_address();
        if net > 0 {
            token_client.transfer(&payer, &contract_addr, &net);
        }
        if treasury_cut > 0 {
            token_client.transfer(&payer, &treasury, &treasury_cut);
        }
        if insurance_cut > 0 {
            token_client.transfer(&payer, &contract_addr, &insurance_cut);
            Self::_add_insurance(&env, insurance_cut)?;
        }
        let pool_key = EscrowDataKey::EscrowPool(pool_id.clone());
        let mut pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&pool_key)
            .ok_or(EscrowError::PoolNotFound)?;
        pool.total_deposited = pool
            .total_deposited
            .checked_add(net)
            .ok_or(EscrowError::Overflow)?;
        env.storage().persistent().set(&pool_key, &pool);
        let fee_record = FeeRecord {
            pool_id: pool_id.clone(),
            sub_type,
            gross_amount,
            fee_amount: fee,
            treasury_cut,
            insurance_cut,
            net_to_escrow: net,
            timestamp: env.ledger().timestamp(),
            payer: payer.clone(),
        };
        env.storage()
            .persistent()
            .set(&EscrowDataKey::FeeRecord(pool_id.clone()), &fee_record);
        FeeCharged {
            pool_id,
            sub_type,
            gross: gross_amount,
            fee,
            treasury_cut,
            insurance_cut,
            net,
        }
        .publish(&env);
        Ok(net)
    }

    pub fn route_pledge(
        env: Env,
        backer: Address,
        pool_id: BytesN<32>,
        pledge_amount: i128,
        asset: Address,
    ) -> Result<i128, EscrowError> {
        backer.require_auth();
        Self::require_not_paused(&env)?;
        if pledge_amount <= 0 {
            return Err(EscrowError::InvalidAmount);
        }
        let config: FeeConfig = env
            .storage()
            .instance()
            .get(&EscrowDataKey::FeeConfig)
            .ok_or(EscrowError::NotInitialized)?;
        let treasury: Address = env
            .storage()
            .instance()
            .get(&EscrowDataKey::Treasury)
            .ok_or(EscrowError::NotInitialized)?;
        let fee = math::calculate_fee_bps(pledge_amount, config.crowdfund_fee_bps)
            .ok_or(EscrowError::Overflow)?;
        let (treasury_cut, insurance_cut) =
            math::split_fee(fee, config.insurance_cut_bps).ok_or(EscrowError::Overflow)?;
        let token_client = token::Client::new(&env, &asset);
        let contract_addr = env.current_contract_address();
        token_client.transfer(&backer, &contract_addr, &pledge_amount);
        if treasury_cut > 0 {
            token_client.transfer(&backer, &treasury, &treasury_cut);
        }
        if insurance_cut > 0 {
            token_client.transfer(&backer, &contract_addr, &insurance_cut);
            Self::_add_insurance(&env, insurance_cut)?;
        }
        let pool_key = EscrowDataKey::EscrowPool(pool_id.clone());
        let mut pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&pool_key)
            .ok_or(EscrowError::PoolNotFound)?;
        pool.total_deposited = pool
            .total_deposited
            .checked_add(pledge_amount)
            .ok_or(EscrowError::Overflow)?;
        env.storage().persistent().set(&pool_key, &pool);
        FeeCharged {
            pool_id: pool_id.clone(),
            sub_type: SubType::CrowdfundPledge,
            gross: pledge_amount
                .checked_add(fee)
                .ok_or(EscrowError::Overflow)?,
            fee,
            treasury_cut,
            insurance_cut,
            net: pledge_amount,
        }
        .publish(&env);
        Ok(pledge_amount)
    }

    /// Convenience wrapper: release escrow to recipient with no fee.
    /// Calls release_partial internally.
    pub fn route_payout(
        env: Env,
        pool_id: BytesN<32>,
        recipient: Address,
        amount: i128,
    ) -> Result<(), EscrowError> {
        Self::release_partial(env, pool_id, recipient, amount)
    }

    /// Convenience wrapper: refund escrowed net amount to pool owner.
    /// Calls refund_all internally.
    pub fn route_refund(env: Env, pool_id: BytesN<32>) -> Result<(), EscrowError> {
        Self::refund_all(env, pool_id)
    }

    // ========================================================================
    // INTERNAL HELPERS
    // ========================================================================

    fn require_admin(env: &Env) -> Result<Address, EscrowError> {
        env.storage()
            .instance()
            .get(&EscrowDataKey::Admin)
            .ok_or(EscrowError::NotInitialized)
    }

    fn require_not_paused(env: &Env) -> Result<(), EscrowError> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&EscrowDataKey::RoutingPaused)
            .unwrap_or(false);
        if paused {
            return Err(EscrowError::RoutingPaused);
        }
        Ok(())
    }

    fn _add_insurance(env: &Env, amount: i128) -> Result<(), EscrowError> {
        let mut fund = env
            .storage()
            .instance()
            .get(&EscrowDataKey::InsuranceFund)
            .unwrap_or(InsuranceFund {
                balance: 0,
                total_contributions: 0,
                total_paid_out: 0,
            });
        fund.balance = fund
            .balance
            .checked_add(amount)
            .ok_or(EscrowError::Overflow)?;
        fund.total_contributions = fund
            .total_contributions
            .checked_add(amount)
            .ok_or(EscrowError::Overflow)?;
        env.storage()
            .instance()
            .set(&EscrowDataKey::InsuranceFund, &fund);
        Ok(())
    }

    fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND);
    }

    fn extend_persistent_ttl(env: &Env, key: &EscrowDataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, PERSISTENT_TTL_THRESHOLD, PERSISTENT_TTL_EXTEND);
    }

    fn compute_pool_id(env: &Env, module: &ModuleType, module_id: u64) -> BytesN<32> {
        let module_byte: u8 = match module {
            ModuleType::Bounty => 0x01,
            ModuleType::Crowdfund => 0x02,
            ModuleType::Grant => 0x03,
            ModuleType::Hackathon => 0x04,
        };
        let mut data = Bytes::new(env);
        data.push_back(module_byte);
        for b in module_id.to_be_bytes() {
            data.push_back(b);
        }
        env.crypto().sha256(&data).into()
    }
}
