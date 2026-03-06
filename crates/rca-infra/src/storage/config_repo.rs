use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::storage::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub monitor_interval_secs: u64,
    pub auto_checkin_enabled: bool,
    pub auto_answer_enabled: bool,
    pub auto_danmu_enabled: bool,
    pub danmu_threshold: usize,
    pub answer_delay_ms: u64,
    pub answer_delay_type: u32,
    pub answer_delay_custom_percent: u32,
    pub notify_enabled: bool,
    pub webhook_url: String,
    pub check_update_on_startup: bool,
    pub active_tenant: TenantKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TenantKind {
    Rain,
    #[default]
    Hetang,
    Yangtze,
    YellowRiver,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            monitor_interval_secs: 5,
            auto_checkin_enabled: true,
            auto_answer_enabled: true,
            auto_danmu_enabled: true,
            danmu_threshold: 4,
            answer_delay_ms: 500,
            answer_delay_type: 1,
            answer_delay_custom_percent: 50,
            notify_enabled: true,
            webhook_url: String::new(),
            check_update_on_startup: true,
            active_tenant: TenantKind::default(),
        }
    }
}

#[async_trait]
pub trait ConfigRepository: Send + Sync {
    async fn load(&self) -> Result<AppConfig, StorageError>;
    async fn save(&self, config: &AppConfig) -> Result<(), StorageError>;
}
