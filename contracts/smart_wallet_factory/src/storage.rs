use soroban_sdk::contracttype;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Whether the factory has been initialized.
    Initialized,
    /// Admin address.
    Admin,
    /// WASM hash of the wallet template contract.
    WalletWasmHash,
    /// Total number of wallets deployed.
    WalletCount,
    /// Wallet address by index (for enumeration).
    Wallet(u32),
    /// Maps owner pubkey hash to wallet address (prevents duplicate deployments).
    OwnerWallet(soroban_sdk::BytesN<32>),
}
