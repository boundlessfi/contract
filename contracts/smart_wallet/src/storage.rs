use soroban_sdk::{contracttype, Address, BytesN};

/// Maximum number of additional signers (beyond the owner).
pub const MAX_SIGNERS: u32 = 10;

/// A signer can be either a native Stellar address or a secp256r1 public key
/// (for passkey/WebAuthn authentication).
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignerKind {
    /// Native Soroban address (G-account or C-account).
    Address(Address),
    /// secp256r1 public key (65 bytes, uncompressed).
    Secp256r1(BytesN<65>),
    /// ed25519 public key (32 bytes).
    Ed25519(BytesN<32>),
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The owner's secp256r1 public key (primary passkey signer).
    OwnerKey,
    /// Number of additional signers.
    SignerCount,
    /// Additional signer by index.
    Signer(u32),
    /// Tracks whether the wallet has been initialized.
    Initialized,
}
