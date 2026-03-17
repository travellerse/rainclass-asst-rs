use std::error::Error;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;

use rca_app::{BootstrapOptions, NotifierMode, StartupActions};
use rca_core::app::{
    AppCommand, AppConfigDto, AppQuery, AppQueryResult, AppService, CoreAppService,
};

use tokio::task::JoinHandle;

#[derive(Default)]
struct TaskGroup {
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl TaskGroup {
    fn spawn<F>(&self, runtime: &tokio::runtime::Runtime, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let handle = runtime.spawn(fut);
        self.handles
            .lock()
            .expect("task group poisoned")
            .push(handle);
    }

    fn abort_all(&self) {
        let handles = std::mem::take(&mut *self.handles.lock().expect("task group poisoned"));
        for handle in handles {
            handle.abort();
        }
    }
}

/// High-level controller that encapsulates the core application service and
/// Tokio runtime.  UI callbacks interact with this object instead of talking
/// directly to `CoreAppService`.
#[derive(Clone)]
pub struct AppController {
    pub app: Arc<CoreAppService>,
    pub runtime: Arc<tokio::runtime::Runtime>,
    tasks: Arc<TaskGroup>,
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
        let runtime = Arc::new(tokio::runtime::Runtime::new()?);
        let app = runtime.block_on(rca_app::bootstrap_core_app(BootstrapOptions {
            default_config: Self::default_config(),
            notifier_mode: NotifierMode::Desktop,
            startup: StartupActions::desktop_default(),
        }))?;

        Ok(AppController {
            app,
            runtime,
            tasks: Arc::new(TaskGroup::default()),
        })
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

    pub fn spawn_task<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.tasks.spawn(&self.runtime, fut);
    }

    pub fn shutdown(&self) {
        self.tasks.abort_all();
        let _ = self
            .runtime
            .block_on(self.app.handle_command(AppCommand::StopMonitor));
        self.app.stop_background_tasks();
    }
}
