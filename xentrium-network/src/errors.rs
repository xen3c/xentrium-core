use thiserror::Error;

/// Network error types
#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("Peer error: {0}")]
    PeerError(String),

    #[error("Discovery error: {0}")]
    DiscoveryError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Timeout error: {0}")]
    TimeoutError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, NetworkError>;
