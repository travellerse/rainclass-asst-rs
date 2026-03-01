use async_trait::async_trait;

use crate::storage::StorageError;

#[async_trait]
pub trait CredentialStore: Send + Sync {
    async fn save_token_pair(
        &self,
        service: &str,
        account: &str,
        access: &str,
        refresh: Option<&str>,
    ) -> Result<(), StorageError>;

    async fn load_token_pair(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Option<(String, Option<String>)>, StorageError>;

    async fn delete_token_pair(&self, service: &str, account: &str) -> Result<(), StorageError>;
}
