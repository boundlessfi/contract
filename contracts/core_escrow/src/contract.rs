use crate::error::Error;
use crate::events::{
    InsuranceClaimed, InsuranceContributed, PoolCreated, PoolLocked, Refunded, SlotReleased,
};
use crate::storage::{DataKey, EscrowPool, InsuranceFund, ModuleType, ReleaseSlot};
use soroban_sdk::{contract, contractimpl, token, xdr::ToXdr, Address, BytesN, Env, Vec};

#[contract]
pub struct CoreEscrow;

#[contractimpl]
impl CoreEscrow {
    pub fn init_core_escrow(
        env: Env,
        admin: Address,
        fee_account: Address,
        treasury: Address,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }

        if Self::is_zero_address(&env, &admin) {
            panic!("admin cannot be zero address");
        }
        if Self::is_zero_address(&env, &fee_account) {
            panic!("fee account cannot be zero address");
        }
        if Self::is_zero_address(&env, &treasury) {
            panic!("treasury cannot be zero address");
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::FeeAccount, &fee_account);
        env.storage().instance().set(&DataKey::Treasury, &treasury);

        Ok(())
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

    pub fn get_pool(env: Env, pool_id: BytesN<32>) -> Result<EscrowPool, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::EscrowPool(pool_id))
            .ok_or(Error::PoolNotFound)
    }

    pub fn get_slot(env: Env, pool_id: BytesN<32>, index: u32) -> Result<ReleaseSlot, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::ReleaseSlot(pool_id, index))
            .ok_or(Error::SlotNotFound)
    }

    pub fn get_all_milestones(env: Env, pool_id: BytesN<32>) -> Result<Vec<ReleaseSlot>, Error> {
        let mut milestones = Vec::new(&env);
        let mut index = 0;
        loop {
            let key = DataKey::ReleaseSlot(pool_id.clone(), index);
            if let Some(slot) = env.storage().persistent().get(&key) {
                milestones.push_back(slot);
                index += 1;
            } else {
                break;
            }
        }
        Ok(milestones)
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
    // CORE ESCROW FUNCTIONS
    // ========================================

    pub fn create_pool(
        env: Env,
        owner: Address,
        module: ModuleType,
        module_id: u64,
        total_amount: i128,
        asset: Address,
        expires_at: u64,
        authorized_caller: Address,
    ) -> Result<BytesN<32>, Error> {
        owner.require_auth();

        let mut data = Vec::new(&env);
        data.push_back(module_id);
        let pool_id: BytesN<32> = env.crypto().sha256(&data.to_xdr(&env)).into();

        if env
            .storage()
            .persistent()
            .has(&DataKey::EscrowPool(pool_id.clone()))
        {
            return Err(Error::PoolAlreadyExists);
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

        env.storage()
            .persistent()
            .set(&DataKey::EscrowPool(pool_id.clone()), &pool);

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
        asset: Address,
        payer: Address,
    ) -> Result<(), Error> {
        payer.require_auth();

        let key = DataKey::EscrowPool(pool_id.clone());
        let mut pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::PoolNotFound)?;

        if pool.locked {
            return Err(Error::PoolLocked);
        }
        if asset != pool.asset {
            return Err(Error::InvalidAsset);
        }

        token::Client::new(&env, &asset).transfer(&payer, &env.current_contract_address(), &amount);

        pool.total_deposited += amount;
        env.storage().persistent().set(&key, &pool);
        Ok(())
    }

    pub fn lock_pool(env: Env, pool_id: BytesN<32>) -> Result<(), Error> {
        let key = DataKey::EscrowPool(pool_id.clone());
        let mut pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::PoolNotFound)?;

        pool.authorized_caller.require_auth();

        if pool.locked {
            return Err(Error::PoolLocked);
        }

        pool.locked = true;
        env.storage().persistent().set(&key, &pool);

        PoolLocked { pool_id }.publish(&env);
        Ok(())
    }

    pub fn define_release_slots(
        env: Env,
        pool_id: BytesN<32>,
        slots: Vec<(Address, i128)>,
    ) -> Result<(), Error> {
        let key = DataKey::EscrowPool(pool_id.clone());
        let pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::PoolNotFound)?;

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
            env.storage()
                .persistent()
                .set(&DataKey::ReleaseSlot(pool_id.clone(), i as u32), &slot);
            total_slot_amount += amount;
        }

        if total_slot_amount > pool.total_deposited {
            return Err(Error::SlotsExceedDeposit);
        }
        Ok(())
    }

    pub fn release_slot(env: Env, pool_id: BytesN<32>, slot_index: u32) -> Result<(), Error> {
        let pool_key = DataKey::EscrowPool(pool_id.clone());
        let mut pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&pool_key)
            .ok_or(Error::PoolNotFound)?;

        pool.authorized_caller.require_auth();

        let slot_key = DataKey::ReleaseSlot(pool_id.clone(), slot_index);
        let mut slot: ReleaseSlot = env
            .storage()
            .persistent()
            .get(&slot_key)
            .ok_or(Error::SlotNotFound)?;

        if slot.released {
            return Err(Error::SlotAlreadyReleased);
        }

        token::Client::new(&env, &pool.asset).transfer(
            &env.current_contract_address(),
            &slot.recipient,
            &slot.amount,
        );

        slot.released = true;
        slot.released_at = Some(env.ledger().timestamp());
        pool.total_released += slot.amount;

        env.storage().persistent().set(&pool_key, &pool);
        env.storage().persistent().set(&slot_key, &slot);

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
    ) -> Result<(), Error> {
        let pool_key = DataKey::EscrowPool(pool_id.clone());
        let mut pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&pool_key)
            .ok_or(Error::PoolNotFound)?;

        pool.authorized_caller.require_auth();

        let remaining = pool.total_deposited - pool.total_released - pool.total_refunded;
        if amount > remaining {
            return Err(Error::InsufficientFunds);
        }

        token::Client::new(&env, &pool.asset).transfer(
            &env.current_contract_address(),
            &recipient,
            &amount,
        );

        pool.total_released += amount;
        env.storage().persistent().set(&pool_key, &pool);

        SlotReleased {
            pool_id,
            slot_index: 999,
            recipient,
            amount,
        }
        .publish(&env);
        Ok(())
    }

    pub fn contribute_insurance(env: Env, amount: i128, asset: Address) -> Result<(), Error> {
        let mut fund = env
            .storage()
            .instance()
            .get(&DataKey::InsuranceFund)
            .unwrap_or(InsuranceFund {
                balance: 0,
                total_contributions: 0,
                total_paid_out: 0,
            });

        fund.balance += amount;
        fund.total_contributions += amount;

        env.storage().instance().set(&DataKey::InsuranceFund, &fund);

        InsuranceContributed { asset, amount }.publish(&env);
        Ok(())
    }

    pub fn claim_insurance(
        env: Env,
        claimant: Address,
        amount: i128,
        asset: Address,
    ) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        token::Client::new(&env, &asset).transfer(
            &env.current_contract_address(),
            &claimant,
            &amount,
        );

        InsuranceClaimed {
            asset,
            claimant,
            amount,
        }
        .publish(&env);
        Ok(())
    }

    pub fn refund_all(env: Env, pool_id: BytesN<32>) -> Result<(), Error> {
        let pool_key = DataKey::EscrowPool(pool_id.clone());
        let mut pool: EscrowPool = env
            .storage()
            .persistent()
            .get(&pool_key)
            .ok_or(Error::PoolNotFound)?;

        pool.authorized_caller.require_auth();

        let remaining = pool.total_deposited - pool.total_released - pool.total_refunded;
        if remaining > 0 {
            token::Client::new(&env, &pool.asset).transfer(
                &env.current_contract_address(),
                &pool.owner,
                &remaining,
            );
            pool.total_refunded += remaining;
            env.storage().persistent().set(&pool_key, &pool);

            Refunded {
                pool_id,
                recipient: pool.owner.clone(),
                amount: remaining,
            }
            .publish(&env);
        }
        Ok(())
    }

    pub fn refund_remaining(env: Env, pool_id: BytesN<32>) -> Result<(), Error> {
        Self::refund_all(env, pool_id)
    }

    // ========================================
    // INTERNAL HELPER FUNCTIONS
    // ========================================

    fn is_zero_address(_env: &Env, _address: &Address) -> bool {
        // In Soroban, there isn't a native "zero address" like EVM.
        // We can check if it's a specific placeholder if needed,
        // but often 'require_auth' is sufficient for security.
        // This is a placeholder implementation as requested.
        false
    }
}
