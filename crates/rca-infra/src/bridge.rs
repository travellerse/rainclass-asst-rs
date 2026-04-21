use std::sync::Arc;

use async_trait::async_trait;

use rca_core::app::ports::{
    ConfigStorePort, NotifierPort, NotifyPortError, SessionStorePort, StoragePortError,
    UpdateCheckerPort, UpdateInfo, UpdatePortError,
};
use rca_core::app::{AppConfigDto, AppNotification};
use rca_core::auth::AuthSession;

use crate::notify::{Notification, Notifier, NotifyLevel};
use crate::storage::{
    AppConfig, ConfigRepository, CredentialStore, SessionRecord, SessionRepository,
};
use crate::update::UpdateChecker;

#[derive(Clone)]
pub struct CoreConfigStoreAdapter {
    inner: Arc<dyn ConfigRepository>,
}

impl CoreConfigStoreAdapter {
    pub fn new(inner: Arc<dyn ConfigRepository>) -> Self {
        Self { inner }
    }
}

#[derive(Clone)]
pub struct CoreSessionStoreAdapter {
    session_repo: Arc<dyn SessionRepository>,
    credential_store: Arc<dyn CredentialStore>,
    service_name: String,
}

impl CoreSessionStoreAdapter {
    const KEYRING_TOKEN_MARKER: &'static str = "__keyring__";

    pub fn new(
        session_repo: Arc<dyn SessionRepository>,
        credential_store: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            session_repo,
            credential_store,
            service_name: "RainClassroomAssistant".to_string(),
        }
    }

    fn account_of(user_id: u64) -> String {
        format!("user:{user_id}")
    }
}

#[derive(Clone)]
pub struct CoreNotifierAdapter {
    inner: Arc<dyn Notifier>,
}

impl CoreNotifierAdapter {
    pub fn new(inner: Arc<dyn Notifier>) -> Self {
        Self { inner }
    }
}

#[derive(Clone)]
pub struct CoreUpdateCheckerAdapter {
    inner: Arc<dyn UpdateChecker>,
}

impl CoreUpdateCheckerAdapter {
    pub fn new(inner: Arc<dyn UpdateChecker>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl ConfigStorePort for CoreConfigStoreAdapter {
    async fn load_config(&self) -> Result<AppConfigDto, StoragePortError> {
        let cfg = self.inner.load().await.map_err(StoragePortError::load)?;
        Ok(AppConfigDto {
            monitor_interval_secs: cfg.monitor_interval_secs,
            auto_checkin_enabled: cfg.auto_checkin_enabled,
            auto_answer_enabled: cfg.auto_answer_enabled,
            auto_answer_random_guess: cfg.auto_answer_random_guess,
            auto_danmu_enabled: cfg.auto_danmu_enabled,
            danmu_threshold: cfg.danmu_threshold,
            answer_delay_ms: cfg.answer_delay_ms,
            answer_delay_type: cfg.answer_delay_type,
            answer_delay_custom_percent: cfg.answer_delay_custom_percent,
            notify_enabled: cfg.notify_enabled,
            notify_events: cfg.notify_events.clone(),
            webhook_url: cfg.webhook_url.clone(),
            check_update_on_startup: cfg.check_update_on_startup,
            tenant: crate::tenant::core_kind_from_storage_kind(cfg.active_tenant),
            auth_state_hint: None,
        })
    }

    async fn save_config(&self, config: &AppConfigDto) -> Result<(), StoragePortError> {
        let cfg = AppConfig {
            monitor_interval_secs: config.monitor_interval_secs,
            auto_checkin_enabled: config.auto_checkin_enabled,
            auto_answer_enabled: config.auto_answer_enabled,
            auto_answer_random_guess: config.auto_answer_random_guess,
            auto_danmu_enabled: config.auto_danmu_enabled,
            danmu_threshold: config.danmu_threshold,
            answer_delay_ms: config.answer_delay_ms,
            answer_delay_type: config.answer_delay_type,
            answer_delay_custom_percent: config.answer_delay_custom_percent,
            notify_enabled: config.notify_enabled,
            notify_events: config.notify_events.clone(),
            webhook_url: config.webhook_url.clone(),
            check_update_on_startup: config.check_update_on_startup,
            active_tenant: crate::tenant::storage_kind_from_core_kind(config.tenant),
        };
        self.inner.save(&cfg).await.map_err(StoragePortError::save)
    }
}

#[async_trait]
impl SessionStorePort for CoreSessionStoreAdapter {
    async fn load_session(&self) -> Result<Option<AuthSession>, StoragePortError> {
        let Some(record) = self
            .session_repo
            .load()
            .await
            .map_err(StoragePortError::load)?
        else {
            return Ok(None);
        };

        let account = Self::account_of(record.user_id);

        // 1. 优先从keyring加载token
        if let Some((access, refresh)) = self
            .credential_store
            .load_token_pair(&self.service_name, &account)
            .await
            .map_err(StoragePortError::load)?
        {
            return Ok(Some(AuthSession {
                user_id: record.user_id,
                access_token: access,
                refresh_token: refresh,
                expires_at_unix_ms: record.expires_at_unix_ms,
                csrf_token: record.csrf_token,
                original_id: record.original_id,
            }));
        }

        // 2. 如果keyring没有，但文件中有实际token（迁移场景）
        // 保存到keyring并更新文件为标记
        if !record.access_token.is_empty() && record.access_token != Self::KEYRING_TOKEN_MARKER {
            // 保存到keyring
            self.credential_store
                .save_token_pair(
                    &self.service_name,
                    &account,
                    &record.access_token,
                    record.refresh_token.as_deref(),
                )
                .await
                .map_err(|e| {
                    tracing::warn!("Failed to migrate token to keyring: {}", e);
                    StoragePortError::save(e)
                })?;

            // 更新session文件，使用keyring标记，不存储实际token
            let sanitized_record = SessionRecord {
                user_id: record.user_id,
                access_token: Self::KEYRING_TOKEN_MARKER.to_string(),
                refresh_token: Some(Self::KEYRING_TOKEN_MARKER.to_string()),
                expires_at_unix_ms: record.expires_at_unix_ms,
                csrf_token: record.csrf_token.clone(),
                original_id: record.original_id.clone(),
            };

            self.session_repo
                .save(&sanitized_record)
                .await
                .map_err(|e| {
                    tracing::warn!("Failed to update session file with keyring marker: {}", e);
                    StoragePortError::save(e)
                })?;

            return Ok(Some(AuthSession {
                user_id: record.user_id,
                access_token: record.access_token,
                refresh_token: record.refresh_token,
                expires_at_unix_ms: record.expires_at_unix_ms,
                csrf_token: record.csrf_token,
                original_id: record.original_id,
            }));
        }

        Ok(None)
    }

    async fn save_session(&self, session: &AuthSession) -> Result<(), StoragePortError> {
        let account = Self::account_of(session.user_id);

        self.credential_store
            .save_token_pair(
                &self.service_name,
                &account,
                &session.access_token,
                session.refresh_token.as_deref(),
            )
            .await
            .map_err(|e| {
                tracing::error!("Failed to save token to keyring: {}", e);
                StoragePortError::save(e)
            })?;

        self.session_repo
            .save(&SessionRecord {
                user_id: session.user_id,
                access_token: Self::KEYRING_TOKEN_MARKER.to_string(),
                refresh_token: Some(Self::KEYRING_TOKEN_MARKER.to_string()),
                expires_at_unix_ms: session.expires_at_unix_ms,
                csrf_token: session.csrf_token.clone(),
                original_id: session.original_id.clone(),
            })
            .await
            .map_err(StoragePortError::save)
    }

    async fn clear_session(&self) -> Result<(), StoragePortError> {
        if let Some(record) = self
            .session_repo
            .load()
            .await
            .map_err(StoragePortError::load)?
        {
            let account = Self::account_of(record.user_id);
            self.credential_store
                .delete_token_pair(&self.service_name, &account)
                .await
                .map_err(StoragePortError::clear)?;
        }
        self.session_repo
            .clear()
            .await
            .map_err(StoragePortError::clear)
    }
}

#[async_trait]
impl NotifierPort for CoreNotifierAdapter {
    async fn notify(&self, message: AppNotification) -> Result<(), NotifyPortError> {
        self.inner
            .notify(Notification {
                id: "app-event".to_string(),
                title: message.title,
                body: message.body,
                level: NotifyLevel::Info,
                created_at: chrono::Utc::now(),
            })
            .await
            .map_err(NotifyPortError::send)
    }
}

#[async_trait]
impl UpdateCheckerPort for CoreUpdateCheckerAdapter {
    async fn check_latest(
        &self,
        current_version: &str,
    ) -> Result<Option<UpdateInfo>, UpdatePortError> {
        let result = self
            .inner
            .check_latest(current_version)
            .await
            .map_err(UpdatePortError::check)?;

        Ok(result.map(|item| UpdateInfo {
            latest_version: item.latest_version,
            release_url: item.release_url,
            published_at_unix_ms: item.published_at_unix_ms,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mockall::mock;
    use rca_core::app::ports::{ConfigStorePort, NotifierPort, SessionStorePort};

    use super::*;
    use crate::notify::NotifyError;
    use crate::storage::StorageError;
    use crate::storage::TenantKind;

    // ── Mocks ──────────────────────────────────────────────────

    mock! {
        ConfigRepo {}
        #[async_trait::async_trait]
        impl ConfigRepository for ConfigRepo {
            async fn load(&self) -> Result<AppConfig, StorageError>;
            async fn save(&self, config: &AppConfig) -> Result<(), StorageError>;
        }
    }

    mock! {
        SessionRepo {}
        #[async_trait::async_trait]
        impl SessionRepository for SessionRepo {
            async fn load(&self) -> Result<Option<SessionRecord>, StorageError>;
            async fn save(&self, session: &SessionRecord) -> Result<(), StorageError>;
            async fn clear(&self) -> Result<(), StorageError>;
        }
    }

    struct FakeCredStore {
        tokens: std::sync::Mutex<Option<(String, Option<String>)>>,
    }

    impl FakeCredStore {
        fn new(tokens: Option<(String, Option<String>)>) -> Self {
            Self {
                tokens: std::sync::Mutex::new(tokens),
            }
        }
    }

    #[async_trait::async_trait]
    impl CredentialStore for FakeCredStore {
        async fn save_token_pair(
            &self,
            _service: &str,
            _account: &str,
            _access: &str,
            _refresh: Option<&str>,
        ) -> Result<(), StorageError> {
            Ok(())
        }
        async fn load_token_pair(
            &self,
            _service: &str,
            _account: &str,
        ) -> Result<Option<(String, Option<String>)>, StorageError> {
            Ok(self.tokens.lock().unwrap().clone())
        }
        async fn delete_token_pair(
            &self,
            _service: &str,
            _account: &str,
        ) -> Result<(), StorageError> {
            *self.tokens.lock().unwrap() = None;
            Ok(())
        }
    }

    mock! {
        TestNotifier {}
        #[async_trait::async_trait]
        impl Notifier for TestNotifier {
            async fn notify(&self, msg: Notification) -> Result<(), NotifyError>;
        }
    }

    // ── CoreConfigStoreAdapter ─────────────────────────────────

    #[tokio::test]
    async fn config_adapter_load_maps_tenant_correctly() {
        let mut mock = MockConfigRepo::new();
        mock.expect_load().returning(|| {
            Ok(AppConfig {
                active_tenant: TenantKind::Rain,
                ..Default::default()
            })
        });

        let adapter = CoreConfigStoreAdapter::new(Arc::new(mock));
        let dto = adapter.load_config().await.unwrap();
        assert_eq!(dto.tenant, rca_core::app::TenantKind::Rain);
    }

    #[tokio::test]
    async fn config_adapter_save_maps_tenant_back() {
        let mut mock = MockConfigRepo::new();
        mock.expect_save()
            .withf(|cfg: &AppConfig| cfg.active_tenant == TenantKind::Yangtze)
            .returning(|_| Ok(()));

        let adapter = CoreConfigStoreAdapter::new(Arc::new(mock));
        let dto = rca_core::app::AppConfigDto {
            tenant: rca_core::app::TenantKind::Yangtze,
            ..Default::default()
        };
        adapter.save_config(&dto).await.unwrap();
    }

    #[tokio::test]
    async fn config_adapter_save_hetang_maps_back() {
        let mut mock = MockConfigRepo::new();
        mock.expect_save()
            .withf(|cfg: &AppConfig| cfg.active_tenant == TenantKind::Hetang)
            .returning(|_| Ok(()));

        let adapter = CoreConfigStoreAdapter::new(Arc::new(mock));
        let dto = rca_core::app::AppConfigDto {
            tenant: rca_core::app::TenantKind::Hetang,
            ..Default::default()
        };
        adapter.save_config(&dto).await.unwrap();
    }

    // ── CoreSessionStoreAdapter ────────────────────────────────

    #[tokio::test]
    async fn session_adapter_load_from_keyring() {
        let mut session_mock = MockSessionRepo::new();
        session_mock.expect_load().returning(|| {
            Ok(Some(SessionRecord {
                user_id: 42,
                access_token: "__keyring__".to_string(),
                refresh_token: None,
                expires_at_unix_ms: Some(99999),
                csrf_token: Some("csrf-keyring".to_string()),
                original_id: Some("orig-keyring".to_string()),
            }))
        });

        let cred = FakeCredStore::new(Some((
            "keyring-access".to_string(),
            Some("keyring-refresh".to_string()),
        )));

        let adapter = CoreSessionStoreAdapter::new(Arc::new(session_mock), Arc::new(cred));
        let session = adapter.load_session().await.unwrap().unwrap();
        assert_eq!(session.access_token, "keyring-access");
        assert_eq!(session.refresh_token, Some("keyring-refresh".to_string()));
        assert_eq!(session.csrf_token.as_deref(), Some("csrf-keyring"));
        assert_eq!(session.original_id.as_deref(), Some("orig-keyring"));
    }

    #[tokio::test]
    async fn session_adapter_load_fallback_to_file_token() {
        let mut session_mock = MockSessionRepo::new();
        session_mock.expect_load().returning(|| {
            Ok(Some(SessionRecord {
                user_id: 42,
                access_token: "file-access".to_string(),
                refresh_token: Some("file-refresh".to_string()),
                expires_at_unix_ms: None,
                csrf_token: Some("csrf-file".to_string()),
                original_id: Some("orig-file".to_string()),
            }))
        });
        session_mock
            .expect_save()
            .withf(|record: &SessionRecord| {
                record.access_token == CoreSessionStoreAdapter::KEYRING_TOKEN_MARKER
                    && record.refresh_token
                        == Some(CoreSessionStoreAdapter::KEYRING_TOKEN_MARKER.to_string())
            })
            .returning(|_| Ok(()));

        let cred = FakeCredStore::new(None);

        let adapter = CoreSessionStoreAdapter::new(Arc::new(session_mock), Arc::new(cred));
        let session = adapter.load_session().await.unwrap().unwrap();
        assert_eq!(session.access_token, "file-access");
        assert_eq!(session.csrf_token.as_deref(), Some("csrf-file"));
        assert_eq!(session.original_id.as_deref(), Some("orig-file"));
    }

    #[tokio::test]
    async fn session_adapter_load_none_when_no_record() {
        let mut session_mock = MockSessionRepo::new();
        session_mock.expect_load().returning(|| Ok(None));

        let cred = FakeCredStore::new(None);

        let adapter = CoreSessionStoreAdapter::new(Arc::new(session_mock), Arc::new(cred));
        let session = adapter.load_session().await.unwrap();
        assert!(session.is_none());
    }

    #[tokio::test]
    async fn session_adapter_clear_deletes_both() {
        let mut session_mock = MockSessionRepo::new();
        session_mock.expect_load().returning(|| {
            Ok(Some(SessionRecord {
                user_id: 42,
                access_token: "tok".to_string(),
                refresh_token: None,
                expires_at_unix_ms: None,
                csrf_token: Some("csrf".to_string()),
                original_id: Some("orig".to_string()),
            }))
        });
        session_mock.expect_clear().returning(|| Ok(()));

        let cred = FakeCredStore::new(Some(("tok".to_string(), None)));

        let adapter = CoreSessionStoreAdapter::new(Arc::new(session_mock), Arc::new(cred));
        adapter.clear_session().await.unwrap();
    }

    // ── CoreNotifierAdapter ────────────────────────────────────

    #[tokio::test]
    async fn notifier_adapter_converts_app_notification() {
        let mut mock = MockTestNotifier::new();
        mock.expect_notify()
            .withf(|msg: &Notification| msg.title == "测试标题" && msg.body == "测试内容")
            .returning(|_| Ok(()));

        let adapter = CoreNotifierAdapter::new(Arc::new(mock));
        adapter
            .notify(AppNotification {
                title: "测试标题".to_string(),
                body: "测试内容".to_string(),
            })
            .await
            .unwrap();
    }
}
