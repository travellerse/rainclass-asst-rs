use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::storage::StorageError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub monitor_interval_secs: u64,
    pub auto_checkin_enabled: bool,
    pub auto_answer_enabled: bool,
    pub auto_answer_random_guess: bool,
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
            auto_answer_random_guess: false,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_config_serde_roundtrip() {
        let config = AppConfig {
            monitor_interval_secs: 10,
            auto_checkin_enabled: false,
            auto_answer_enabled: true,
            auto_answer_random_guess: true,
            auto_danmu_enabled: false,
            danmu_threshold: 8,
            answer_delay_ms: 2000,
            answer_delay_type: 3,
            answer_delay_custom_percent: 75,
            notify_enabled: false,
            webhook_url: "https://hook.example.com".to_string(),
            check_update_on_startup: false,
            active_tenant: TenantKind::Rain,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.monitor_interval_secs, 10);
        assert!(!deserialized.auto_checkin_enabled);
        assert!(deserialized.auto_answer_random_guess);
        assert_eq!(deserialized.active_tenant, TenantKind::Rain);
        assert_eq!(deserialized.webhook_url, "https://hook.example.com");
    }

    #[test]
    fn app_config_serde_default_fills_missing_fields() {
        // JSON with only a subset of fields — serde(default) should fill the rest
        let json = r#"{"monitor_interval_secs": 99}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.monitor_interval_secs, 99);
        // All other fields should have defaults
        assert!(config.auto_checkin_enabled);
        assert!(config.auto_answer_enabled);
        assert!(!config.auto_answer_random_guess);
        assert_eq!(config.active_tenant, TenantKind::Hetang);
    }

    #[test]
    fn tenant_kind_default_is_hetang() {
        assert_eq!(TenantKind::default(), TenantKind::Hetang);
    }

    #[test]
    fn tenant_kind_serde_roundtrip() {
        for kind in [
            TenantKind::Rain,
            TenantKind::Hetang,
            TenantKind::Yangtze,
            TenantKind::YellowRiver,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            let deserialized: TenantKind = serde_json::from_str(&json).unwrap();
            assert_eq!(deserialized, kind);
        }
    }
}
