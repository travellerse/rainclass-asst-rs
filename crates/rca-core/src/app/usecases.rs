use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::app::{AppCommand, AppError, AppEvent, AppQuery, AppState};

#[derive(Debug, Clone)]
pub enum AppQueryResult {
    State(AppState),
    Config(crate::app::AppConfigDto),
    Events(Vec<crate::monitor::CoreEvent>),
}

#[async_trait]
pub trait AppService: Send + Sync {
    async fn handle_command(&self, cmd: AppCommand) -> Result<(), AppError>;
    async fn handle_query(&self, query: AppQuery) -> Result<AppQueryResult, AppError>;
    async fn subscribe_events(&self) -> mpsc::Receiver<AppEvent>;
}
