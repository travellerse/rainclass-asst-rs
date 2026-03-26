use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::storage::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub user_id: u64,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_unix_ms: Option<i64>,
    pub csrf_token: Option<String>,
    pub original_id: Option<String>,
}

#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn load(&self) -> Result<Option<SessionRecord>, StorageError>;
    async fn save(&self, session: &SessionRecord) -> Result<(), StorageError>;
    async fn clear(&self) -> Result<(), StorageError>;
}
