use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tracing::info;

use crate::notify::NotifyError;

#[derive(Debug, Clone)]
pub enum NotifyLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: String,
    pub title: String,
    pub body: String,
    pub level: NotifyLevel,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait Notifier: Send + Sync {
    async fn notify(&self, msg: Notification) -> Result<(), NotifyError>;
}

#[derive(Debug, Default, Clone)]
pub struct LoggingNotifier;

#[async_trait]
impl Notifier for LoggingNotifier {
    async fn notify(&self, msg: Notification) -> Result<(), NotifyError> {
        info!(
            target: "rca.notify",
            id = %msg.id,
            title = %msg.title,
            level = ?msg.level,
            body = %msg.body,
            "desktop notification"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::NotifyLevel;
    use chrono::Utc;

    #[tokio::test]
    async fn test_logging_notifier_success() {
        let notifier = LoggingNotifier;
        let notification = Notification {
            id: "test".to_string(),
            title: "Title".to_string(),
            body: "Body".to_string(),
            level: NotifyLevel::Info,
            created_at: Utc::now(),
        };

        let result = notifier.notify(notification).await;
        assert!(result.is_ok());
    }
}
