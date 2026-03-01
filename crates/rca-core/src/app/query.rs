#[derive(Debug, Clone)]
pub enum AppQuery {
    GetAppState,
    GetConfig,
    GetRecentEvents { limit: usize },
}
