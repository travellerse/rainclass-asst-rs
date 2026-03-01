use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::storage::{CredentialStore, StorageError};

#[derive(Debug, Default)]
pub struct KeyringCredentialStore;

#[derive(Debug, Serialize, Deserialize)]
struct TokenPair {
    access: String,
    refresh: Option<String>,
}

#[async_trait]
impl CredentialStore for KeyringCredentialStore {
    async fn save_token_pair(
        &self,
        service: &str,
        account: &str,
        access: &str,
        refresh: Option<&str>,
    ) -> Result<(), StorageError> {
        let entry = keyring::Entry::new(service, account).map_err(|source| StorageError::KeyringInit {
            service: service.to_string(),
            account: account.to_string(),
            source,
        })?;
        let payload = TokenPair {
            access: access.to_string(),
            refresh: refresh.map(ToString::to_string),
        };
        let raw = serde_json::to_string(&payload)?;
        entry.set_password(&raw).map_err(|source| StorageError::KeyringSet {
            service: service.to_string(),
            account: account.to_string(),
            source,
        })?;
        Ok(())
    }

    async fn load_token_pair(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Option<(String, Option<String>)>, StorageError> {
        let entry = keyring::Entry::new(service, account).map_err(|source| StorageError::KeyringInit {
            service: service.to_string(),
            account: account.to_string(),
            source,
        })?;
        match entry.get_password() {
            Ok(raw) => {
                let pair: TokenPair = serde_json::from_str(&raw)?;
                Ok(Some((pair.access, pair.refresh)))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(source) => Err(StorageError::KeyringGet {
                service: service.to_string(),
                account: account.to_string(),
                source,
            }),
        }
    }

    async fn delete_token_pair(&self, service: &str, account: &str) -> Result<(), StorageError> {
        let entry = keyring::Entry::new(service, account).map_err(|source| StorageError::KeyringInit {
            service: service.to_string(),
            account: account.to_string(),
            source,
        })?;
        match entry.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(source) => Err(StorageError::KeyringDelete {
                service: service.to_string(),
                account: account.to_string(),
                source,
            }),
        }
    }
}
