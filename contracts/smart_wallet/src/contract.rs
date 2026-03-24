use crate::error::Error;
use crate::storage::{DataKey, SignerKind, MAX_SIGNERS};
use soroban_sdk::{
    auth::{Context, CustomAccountInterface},
    contract, contractimpl,
    crypto::Hash,
    BytesN, Env, Vec,
};

#[contract]
pub struct SmartWallet;

/// The signature payload sent during `__check_auth`.
/// Each element is a (signer_kind_tag, signature_bytes) tuple.
/// - For secp256r1: tag=0, sig=BytesN<64>
/// - For ed25519: tag=1, sig=BytesN<64>
/// - For native address: tag=2, no sig needed (address auth is handled by the runtime)

#[contractimpl]
impl SmartWallet {
    // ========================================================================
    // INITIALIZATION
    // ========================================================================

    /// Initialize the wallet with a secp256r1 public key (passkey).
    pub fn init(env: Env, owner_pk: BytesN<65>) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Initialized) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage()
            .instance()
            .set(&DataKey::OwnerKey, &owner_pk);
        env.storage().instance().set(&DataKey::SignerCount, &0u32);
        env.storage().instance().set(&DataKey::Initialized, &true);
        Ok(())
    }

    // ========================================================================
    // SIGNER MANAGEMENT (requires wallet auth — self-invocation)
    // ========================================================================

    /// Add an additional signer. Requires wallet authorization.
    pub fn add_signer(env: Env, signer: SignerKind) -> Result<(), Error> {
        env.current_contract_address().require_auth();

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::SignerCount)
            .unwrap_or(0);

        if count >= MAX_SIGNERS {
            return Err(Error::TooManySigners);
        }

        // Check for duplicates
        for i in 0..count {
            let existing: SignerKind =
                env.storage().instance().get(&DataKey::Signer(i)).unwrap();
            if existing == signer {
                return Err(Error::SignerAlreadyExists);
            }
        }

        env.storage()
            .instance()
            .set(&DataKey::Signer(count), &signer);
        env.storage()
            .instance()
            .set(&DataKey::SignerCount, &(count + 1));
        Ok(())
    }

    /// Remove an additional signer by index. Requires wallet authorization.
    pub fn remove_signer(env: Env, index: u32) -> Result<(), Error> {
        env.current_contract_address().require_auth();

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::SignerCount)
            .unwrap_or(0);

        if index >= count {
            return Err(Error::SignerNotFound);
        }

        // Move last signer to the removed slot (swap-remove)
        if index < count - 1 {
            let last: SignerKind = env
                .storage()
                .instance()
                .get(&DataKey::Signer(count - 1))
                .unwrap();
            env.storage()
                .instance()
                .set(&DataKey::Signer(index), &last);
        }
        env.storage()
            .instance()
            .remove(&DataKey::Signer(count - 1));
        env.storage()
            .instance()
            .set(&DataKey::SignerCount, &(count - 1));
        Ok(())
    }

    // ========================================================================
    // QUERIES
    // ========================================================================

    pub fn get_owner_pk(env: Env) -> Result<BytesN<65>, Error> {
        env.storage()
            .instance()
            .get(&DataKey::OwnerKey)
            .ok_or(Error::NotInitialized)
    }

    pub fn get_signer_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::SignerCount)
            .unwrap_or(0)
    }

    pub fn get_signer(env: Env, index: u32) -> Result<SignerKind, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Signer(index))
            .ok_or(Error::SignerNotFound)
    }

    // ========================================================================
    // UPGRADE
    // ========================================================================

    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        env.current_contract_address().require_auth();
        env.deployer().update_current_contract_wasm(new_wasm_hash);
        Ok(())
    }
}

#[contractimpl]
impl CustomAccountInterface for SmartWallet {
    type Error = Error;
    type Signature = Vec<BytesN<64>>;

    /// Verify signatures for this smart wallet.
    ///
    /// The `signature` parameter is a Vec of secp256r1 signatures (64 bytes each).
    /// We try to match each signature against the owner key and all additional
    /// secp256r1/ed25519 signers. At least one valid signature is required.
    ///
    /// For native Address signers, the runtime handles their auth separately —
    /// they don't need to appear in this signature list.
    #[allow(non_snake_case)]
    fn __check_auth(
        env: Env,
        signature_payload: Hash<32>,
        signatures: Vec<BytesN<64>>,
        _auth_contexts: Vec<Context>,
    ) -> Result<(), Error> {
        if signatures.is_empty() {
            return Err(Error::NoValidSignature);
        }

        let payload_bytes = signature_payload.into();

        // Try owner key first
        let owner_pk: BytesN<65> = env
            .storage()
            .instance()
            .get(&DataKey::OwnerKey)
            .ok_or(Error::NotInitialized)?;

        for sig in signatures.iter() {
            // Try secp256r1 verify against owner
            let result = env
                .crypto()
                .secp256r1_verify(&owner_pk, &payload_bytes, &sig);
            // secp256r1_verify panics on failure, so if we reach here it's valid
            let _ = result;
            return Ok(());
        }

        // If owner didn't match, try additional signers
        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::SignerCount)
            .unwrap_or(0);

        for sig in signatures.iter() {
            for i in 0..count {
                if let Some(signer) = env
                    .storage()
                    .instance()
                    .get::<_, SignerKind>(&DataKey::Signer(i))
                {
                    match &signer {
                        SignerKind::Secp256r1(pk) => {
                            // Try verification — panics on invalid sig
                            env.crypto().secp256r1_verify(pk, &payload_bytes, &sig);
                            return Ok(());
                        }
                        SignerKind::Ed25519(pk) => {
                            env.crypto().ed25519_verify(pk, &payload_bytes.clone().into(), &sig);
                            return Ok(());
                        }
                        SignerKind::Address(_) => {
                            // Address signers are handled by the runtime
                            continue;
                        }
                    }
                }
            }
        }

        Err(Error::NoValidSignature)
    }
}
