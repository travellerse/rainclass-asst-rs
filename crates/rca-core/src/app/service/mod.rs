use std::sync::{Arc, Mutex};

mod api;
mod background;
mod impls;

use tokio::sync::mpsc;

use crate::app::ports::{
    ApiPort, ConfigStorePort, NotifierPort, SessionStorePort, UpdateCheckerPort,
};
use crate::app::{AppConfigDto, AppEvent, AppState};
use crate::monitor::MonitorHandle;

pub(super) const MAX_RECENT_EVENTS: usize = 200;

#[derive(Clone)]
pub struct CoreAppDeps {
    pub api: Arc<dyn ApiPort>,
    pub config_store: Arc<dyn ConfigStorePort>,
    pub session_store: Arc<dyn SessionStorePort>,
    pub notifier: Arc<dyn NotifierPort>,
    pub update_checker: Arc<dyn UpdateCheckerPort>,
    pub monitor_engine: Arc<dyn crate::monitor::MonitorEngine>,
}

pub(super) struct InnerState {
    app_state: AppState,
    config: AppConfigDto,
    monitor_handle: Option<MonitorHandle>,
    subscribers: Vec<mpsc::Sender<AppEvent>>,
}

pub struct AppServiceImpl {
    deps: CoreAppDeps,
    inner: Arc<Mutex<InnerState>>,
    background: Mutex<Option<background::CoreBackgroundTasks>>,
}

#[cfg(test)]
mod tests;
