use std::error::Error;
use std::sync::Arc;

use rca_core::app::{AppCommand, AppConfigDto, AppService, AppServiceImpl, CoreAppDeps};
use rca_infra::storage::ConfigRepository;

#[derive(Debug, Clone, Copy)]
pub enum NotifierMode {
    Cli,
    Desktop,
}

#[derive(Debug, Clone, Copy)]
pub struct StartupActions {
    pub load_config: bool,
    pub restore_session: bool,
    pub refresh_session: bool,
}

impl StartupActions {
    pub fn cli_default() -> Self {
        Self {
            load_config: true,
            restore_session: true,
            refresh_session: false,
        }
    }

    pub fn desktop_default() -> Self {
        Self {
            load_config: true,
            restore_session: true,
            refresh_session: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BootstrapOptions {
    pub default_config: AppConfigDto,
    pub notifier_mode: NotifierMode,
    pub startup: StartupActions,
}

#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("bootstrap failed: {0}")]
    Bootstrap(#[from] Box<dyn Error + Send + Sync>),
}

pub fn init_default_logger(
    default_level: &str,
) -> Vec<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = match rca_infra::storage::AppPaths::detect() {
        Ok(paths) => paths.log_dir,
        Err(_) => std::env::current_dir().unwrap_or_default().join("logs"),
    };
    rca_infra::log::init_logger(log_dir, default_level)
}

pub async fn bootstrap_core_app(
    options: BootstrapOptions,
) -> Result<Arc<AppServiceImpl>, BootstrapError> {
    let app = build_core_app(&options).await?;
    run_startup_actions(app.clone(), options.startup).await?;
    Ok(app)
}

async fn build_core_app(options: &BootstrapOptions) -> Result<Arc<AppServiceImpl>, BootstrapError> {
    let paths = rca_infra::storage::AppPaths::detect().map_err(boxed)?;
    let config_repo = Arc::new(rca_infra::storage::JsonFileConfigRepository::new(
        paths.config_file,
    ));
    let session_repo = Arc::new(rca_infra::storage::JsonFileSessionRepository::new(
        paths.session_file,
    ));
    let credential_store = Arc::new(rca_infra::storage::KeyringCredentialStore);

    let initial_config = config_repo.load().await.ok();
    let initial_tenant = initial_config
        .as_ref()
        .map(|cfg| rca_infra::tenant::tenant_host_from_kind(cfg.active_tenant))
        .unwrap_or(rca_infra::api::TenantHost::Hetang);

    let notifier: Arc<dyn rca_infra::notify::Notifier> = match options.notifier_mode {
        NotifierMode::Cli => Arc::new(rca_infra::notify::LoggingNotifier),
        NotifierMode::Desktop => {
            let notifiers: Vec<Box<dyn rca_infra::notify::Notifier>> = vec![
                Box::new(rca_infra::notify::LoggingNotifier),
                Box::new(rca_infra::notify::DesktopNotifier::new()),
                Box::new(rca_infra::notify::ConfigWebhookNotifier::new(
                    config_repo.clone(),
                )),
            ];
            Arc::new(rca_infra::notify::MultiNotifier::new(notifiers))
        }
    };

    let update_checker = Arc::new(
        rca_infra::update::GithubReleaseChecker::new("travellerse", "RainClassroomAssistant")
            .map_err(boxed)?,
    );

    let api_port: Arc<dyn rca_core::app::ports::ApiPort> = Arc::new(
        rca_infra::api::YktApiPort::new(rca_infra::api::YktApiPortConfig {
            tenant: initial_tenant,
            timeout_secs: 15,
        })
        .map_err(boxed)?,
    );

    let monitor = Arc::new(rca_core::monitor::CoreMonitorEngine::new(api_port.clone()));

    let app = Arc::new(AppServiceImpl::new_started(
        CoreAppDeps {
            api: api_port,
            config_store: Arc::new(rca_infra::bridge::CoreConfigStoreAdapter::new(config_repo)),
            session_store: Arc::new(rca_infra::bridge::CoreSessionStoreAdapter::new(
                session_repo,
                credential_store,
            )),
            notifier: Arc::new(rca_infra::bridge::CoreNotifierAdapter::new(notifier)),
            update_checker: Arc::new(rca_infra::bridge::CoreUpdateCheckerAdapter::new(
                update_checker,
            )),
            monitor_engine: monitor,
        },
        options.default_config.clone(),
    ));

    Ok(app)
}

async fn run_startup_actions(
    app: Arc<AppServiceImpl>,
    startup: StartupActions,
) -> Result<(), BootstrapError> {
    if startup.load_config {
        let _ = app.handle_command(AppCommand::LoadConfig).await;
    }
    if startup.restore_session {
        let _ = app.handle_command(AppCommand::RestoreSession).await;
    }
    if startup.refresh_session {
        let _ = app.handle_command(AppCommand::RefreshSession).await;
    }
    Ok(())
}

fn boxed<E: Error + Send + Sync + 'static>(err: E) -> BootstrapError {
    BootstrapError::Bootstrap(Box::new(err))
}
