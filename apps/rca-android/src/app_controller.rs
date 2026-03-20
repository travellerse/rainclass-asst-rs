use std::error::Error;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use rca_app::{BootstrapOptions, NotifierMode, StartupActions};
use rca_core::app::{
    AppCommand, AppConfigDto, AppQuery, AppQueryResult, AppService, AppServiceImpl,
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

#[derive(Clone)]
pub struct AndroidController {
    pub app: Arc<AppServiceImpl>,
    pub runtime: Arc<tokio::runtime::Runtime>,
    tasks: Arc<TaskGroup>,
}

impl AndroidController {
    pub fn default_config() -> AppConfigDto {
        AppConfigDto::default()
    }

    pub fn bootstrap(storage_root: Option<PathBuf>) -> Result<Self, Box<dyn Error>> {
        let runtime = Arc::new(tokio::runtime::Runtime::new()?);
        let app = runtime.block_on(rca_app::bootstrap_core_app(BootstrapOptions {
            default_config: Self::default_config(),
            notifier_mode: NotifierMode::Android,
            startup: StartupActions::mobile_default(),
            storage_root,
        }))?;

        Ok(Self {
            app,
            runtime,
            tasks: Arc::new(TaskGroup::default()),
        })
    }

    pub async fn get_state(&self) -> Result<AppQueryResult, rca_core::app::AppError> {
        self.app.handle_query(AppQuery::GetAppState).await
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
