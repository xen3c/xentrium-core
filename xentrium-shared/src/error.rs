use thiserror::Error;

pub type Result<T> = std::result::Result<T, XentriumError>;

#[derive(Error, Debug)]
pub enum XentriumError {
    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Insufficient balance")]
    InsufficientBalance,

    #[error("Invalid nonce")]
    InvalidNonce,

    #[error("Wallet locked")]
    WalletLocked,

    #[error("Invalid password")]
    InvalidPassword,

    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Validator error: {0}")]
    ValidatorError(String),

    #[error("Consensus error: {0}")]
    ConsensusError(String),

    #[error("State error: {0}")]
    StateError(String),

    #[error("Governance error: {0}")]
    GovernanceError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    ConfigError(String),
}
