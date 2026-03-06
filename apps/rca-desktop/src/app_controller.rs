use std::error::Error;
use std::sync::Arc;

use rca_core::app::{
    AppCommand, AppConfigDto, AppQuery, AppQueryResult, AppService, CoreAppService,
};
use rca_infra::storage::ConfigRepository;

/// High-level controller that encapsulates the core application service and
/// Tokio runtime.  UI callbacks interact with this object instead of talking
/// directly to `CoreAppService`.
#[derive(Clone)]
pub struct AppController {
    pub app: Arc<CoreAppService>,
    pub runtime: Arc<tokio::runtime::Runtime>,
}

impl AppController {
    /// Application default configuration used when no existing config is found.
    ///
    /// Copied from the original `main.rs` helper.
    pub fn default_config() -> AppConfigDto {
        AppConfigDto::default()
    }

    /// Bootstraps the dependencies and returns a ready-to-use controller.
    pub fn bootstrap() -> Result<Self, Box<dyn Error>> {
        // replicate initialization logic from original main.rs
        let runtime = Arc::new(tokio::runtime::Runtime::new()?);

        let paths = rca_infra::storage::AppPaths::detect()?;
        let config_repo = Arc::new(rca_infra::storage::JsonFileConfigRepository::new(
            paths.config_file,
        ));
        let session_repo = Arc::new(rca_infra::storage::JsonFileSessionRepository::new(
            paths.session_file,
        ));
        let credential_store = Arc::new(rca_infra::storage::KeyringCredentialStore);

        let initial_config = runtime.block_on(config_repo.load()).ok();

        let initial_tenant = initial_config
            .as_ref()
            .map(|cfg| match cfg.active_tenant {
                rca_infra::storage::TenantKind::Rain => rca_infra::api::TenantHost::Rain,
                rca_infra::storage::TenantKind::Hetang => rca_infra::api::TenantHost::Hetang,
                rca_infra::storage::TenantKind::Yangtze => rca_infra::api::TenantHost::Yangtze,
                rca_infra::storage::TenantKind::YellowRiver => {
                    rca_infra::api::TenantHost::YellowRiver
                }
            })
            .unwrap_or(rca_infra::api::TenantHost::Hetang);

        let mut notifiers: Vec<Box<dyn rca_infra::notify::Notifier>> = vec![
            Box::new(rca_infra::notify::LoggingNotifier),
            Box::new(rca_infra::notify::DesktopNotifier::new()),
        ];

        if let Some(cfg) = initial_config.as_ref()
            && !cfg.webhook_url.is_empty()
        {
            notifiers.push(Box::new(rca_infra::notify::WebhookNotifier::new(
                &cfg.webhook_url,
            )));
        }

        let notifier = Arc::new(rca_infra::notify::MultiNotifier::new(notifiers));

        let update_checker = Arc::new(rca_infra::update::GithubReleaseChecker::new(
            "travellerse",
            "RainClassroomAssistant",
        )?);

        let api_port: Arc<dyn rca_core::app::ports::ApiPort> = Arc::new(
            rca_infra::api::YktApiPort::new(rca_infra::api::YktApiPortConfig {
                tenant: initial_tenant,
                timeout_secs: 15,
            })?,
        );

        let config_port = Arc::new(rca_infra::bridge::CoreConfigStoreAdapter::new(config_repo));
        let session_port = Arc::new(rca_infra::bridge::CoreSessionStoreAdapter::new(
            session_repo,
            credential_store,
        ));
        let notify_port = Arc::new(rca_infra::bridge::CoreNotifierAdapter::new(notifier));
        let update_port = Arc::new(rca_infra::bridge::CoreUpdateCheckerAdapter::new(
            update_checker,
        ));

        let monitor = Arc::new(rca_core::monitor::CoreMonitorEngine::new(api_port.clone()));

        let app = {
            let _guard = runtime.enter();
            Arc::new(CoreAppService::new(
                rca_core::app::CoreAppDeps {
                    api: api_port,
                    config_store: config_port,
                    session_store: session_port,
                    notifier: notify_port,
                    update_checker: update_port,
                    monitor_engine: monitor,
                },
                Self::default_config(),
            ))
        };

        // bootstrap commands
        let _ = runtime.block_on(app.handle_command(AppCommand::LoadConfig));
        let _ = runtime.block_on(app.handle_command(AppCommand::RestoreSession));
        let _ = runtime.block_on(app.handle_command(AppCommand::RefreshSession));

        Ok(AppController { app, runtime })
    }

    /// Query current application state.
    pub async fn get_state(&self) -> Result<AppQueryResult, rca_core::app::AppError> {
        self.app.handle_query(AppQuery::GetAppState).await
    }

    pub async fn get_config(&self) -> Result<AppQueryResult, rca_core::app::AppError> {
        self.app.handle_query(AppQuery::GetConfig).await
    }

    pub async fn login_by_qr(&self) -> Result<(), rca_core::app::AppError> {
        self.app.handle_command(AppCommand::LoginByQr).await
    }

    pub async fn check_update(&self) -> Result<(), rca_core::app::AppError> {
        self.app.handle_command(AppCommand::CheckUpdate).await
    }
}
