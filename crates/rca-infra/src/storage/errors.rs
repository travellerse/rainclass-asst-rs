use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialize error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("keyring init failed ({service}/{account}): {source}")]
    KeyringInit {
        service: String,
        account: String,
        #[source]
        source: keyring_core::Error,
    },

    #[error("keyring set failed ({service}/{account}): {source}")]
    KeyringSet {
        service: String,
        account: String,
        #[source]
        source: keyring_core::Error,
    },

    #[error("keyring get failed ({service}/{account}): {source}")]
    KeyringGet {
        service: String,
        account: String,
        #[source]
        source: keyring_core::Error,
    },

    #[error("keyring delete failed ({service}/{account}): {source}")]
    KeyringDelete {
        service: String,
        account: String,
        #[source]
        source: keyring_core::Error,
    },

    #[error("invalid config: {0}")]
    InvalidConfig(String),
}
