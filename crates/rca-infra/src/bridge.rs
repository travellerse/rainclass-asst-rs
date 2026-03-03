use std::sync::Arc;

use async_trait::async_trait;

use rca_core::app::ports::{
    ConfigStorePort, NotifierPort, NotifyPortError, SessionStorePort,
    StoragePortError, UpdateCheckerPort, UpdateInfo, UpdatePortError,
};
use rca_core::app::{AppConfigDto, AppNotification};
use rca_core::auth::AuthSession;

use crate::notify::{Notification, Notifier, NotifyLevel};
use crate::storage::{
    AppConfig, ConfigRepository, CredentialStore, SessionRecord, SessionRepository, TenantKind,
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
        let cfg = self
            .inner
            .load()
            .await
            .map_err(StoragePortError::load)?;
        Ok(AppConfigDto {
            monitor_interval_secs: cfg.monitor_interval_secs,
            auto_checkin_enabled: cfg.auto_checkin_enabled,
            auto_answer_enabled: cfg.auto_answer_enabled,
            answer_delay_ms: cfg.answer_delay_ms,
            notify_enabled: cfg.notify_enabled,
            check_update_on_startup: cfg.check_update_on_startup,
            auth_state_hint: None,
        })
    }

    async fn save_config(&self, config: &AppConfigDto) -> Result<(), StoragePortError> {
        let cfg = AppConfig {
            monitor_interval_secs: config.monitor_interval_secs,
            auto_checkin_enabled: config.auto_checkin_enabled,
            auto_answer_enabled: config.auto_answer_enabled,
            answer_delay_ms: config.answer_delay_ms,
            notify_enabled: config.notify_enabled,
            check_update_on_startup: config.check_update_on_startup,
            active_tenant: TenantKind::Rain,
        };
        self.inner
            .save(&cfg)
            .await
            .map_err(StoragePortError::save)
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
            }));
        }

        if !record.access_token.is_empty() && record.access_token != "__keyring__" {
            let _ = self
                .credential_store
                .save_token_pair(
                    &self.service_name,
                    &account,
                    &record.access_token,
                    record.refresh_token.as_deref(),
                )
                .await;

            return Ok(Some(AuthSession {
                user_id: record.user_id,
                access_token: record.access_token,
                refresh_token: record.refresh_token,
                expires_at_unix_ms: record.expires_at_unix_ms,
            }));
        }

        Ok(None)
    }

    async fn save_session(&self, session: &AuthSession) -> Result<(), StoragePortError> {
        let account = Self::account_of(session.user_id);
        let _ = self
            .credential_store
            .save_token_pair(
                &self.service_name,
                &account,
                &session.access_token,
                session.refresh_token.as_deref(),
            )
            .await;

        self.session_repo
            .save(&SessionRecord {
                user_id: session.user_id,
                access_token: session.access_token.clone(),
                refresh_token: session.refresh_token.clone(),
                expires_at_unix_ms: session.expires_at_unix_ms,
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
    async fn check_latest(&self, current_version: &str) -> Result<Option<UpdateInfo>, UpdatePortError> {
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

