use crate::error::Error;
use crate::storage::DataKey;
use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

#[contract]
pub struct SmartWalletFactory;

#[contractimpl]
impl SmartWalletFactory {
    /// Initialize the factory with an admin and the WASM hash of the wallet template.
    pub fn init(env: Env, admin: Address, wallet_wasm_hash: BytesN<32>) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Initialized) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::WalletWasmHash, &wallet_wasm_hash);
        env.storage().instance().set(&DataKey::WalletCount, &0u32);
        env.storage().instance().set(&DataKey::Initialized, &true);
        Ok(())
    }

    /// Deploy a new smart wallet for the given owner secp256r1 public key.
    /// Returns the deployed wallet's contract address.
    pub fn deploy_wallet(env: Env, owner_pk: BytesN<65>) -> Result<Address, Error> {
        if !env.storage().instance().has(&DataKey::Initialized) {
            return Err(Error::NotInitialized);
        }

        // Derive a deterministic salt from the owner's public key
        let salt: BytesN<32> = env.crypto().sha256(&owner_pk.clone().into()).into();

        // Check if wallet already deployed for this owner
        let owner_key = DataKey::OwnerWallet(salt.clone());
        if env.storage().persistent().has(&owner_key) {
            return Err(Error::DeployFailed);
        }

        let wasm_hash: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::WalletWasmHash)
            .ok_or(Error::NotInitialized)?;

        // Deploy the wallet contract
        let wallet_address = env
            .deployer()
            .with_current_contract(salt)
            .deploy_v2(wasm_hash, ());

        // Initialize the wallet with the owner's public key
        // We call init on the deployed contract
        let init_fn = soroban_sdk::Symbol::new(&env, "init");
        let args = soroban_sdk::vec![&env, owner_pk.to_val()];
        let _: soroban_sdk::Val = env.invoke_contract(&wallet_address, &init_fn, args);

        // Record the wallet
        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::WalletCount)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::Wallet(count), &wallet_address);
        env.storage()
            .instance()
            .set(&DataKey::WalletCount, &(count + 1));

        // Map owner to wallet
        let owner_hash: BytesN<32> = env.crypto().sha256(&owner_pk.into()).into();
        env.storage()
            .persistent()
            .set(&DataKey::OwnerWallet(owner_hash), &wallet_address);

        Ok(wallet_address)
    }

    /// Update the wallet template WASM hash. Admin only.
    pub fn upgrade_template(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::WalletWasmHash, &new_wasm_hash);
        Ok(())
    }

    /// Upgrade the factory contract itself.
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

    // ========================================================================
    // QUERIES
    // ========================================================================

    /// Get the total number of deployed wallets.
    pub fn get_wallet_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::WalletCount)
            .unwrap_or(0)
    }

    /// Get a wallet address by index.
    pub fn get_wallet(env: Env, index: u32) -> Result<Address, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Wallet(index))
            .ok_or(Error::NotInitialized)
    }

    /// Get the wallet address for a given owner public key.
    pub fn get_wallet_by_owner(env: Env, owner_pk: BytesN<65>) -> Result<Address, Error> {
        let owner_hash: BytesN<32> = env.crypto().sha256(&owner_pk.into()).into();
        env.storage()
            .persistent()
            .get(&DataKey::OwnerWallet(owner_hash))
            .ok_or(Error::NotInitialized)
    }

    /// Get the current wallet WASM hash.
    pub fn get_wasm_hash(env: Env) -> Result<BytesN<32>, Error> {
        env.storage()
            .instance()
            .get(&DataKey::WalletWasmHash)
            .ok_or(Error::NotInitialized)
    }
}
