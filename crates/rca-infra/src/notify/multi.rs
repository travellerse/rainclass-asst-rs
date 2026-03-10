use async_trait::async_trait;
use tracing::error;

use crate::notify::{Notification, Notifier, NotifyError};

pub struct MultiNotifier {
    notifiers: Vec<Box<dyn Notifier>>,
}

impl MultiNotifier {
    pub fn new(notifiers: Vec<Box<dyn Notifier>>) -> Self {
        Self { notifiers }
    }
}

#[async_trait]
impl Notifier for MultiNotifier {
    async fn notify(&self, msg: Notification) -> Result<(), NotifyError> {
        for notifier in &self.notifiers {
            if let Err(e) = notifier.notify(msg.clone()).await {
                error!("One of the notifiers failed: {}", e);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::NotifyLevel;
    use chrono::Utc;
    use mockall::mock;
    use mockall::predicate;

    mock! {
        pub Notifier {}
        #[async_trait]
        impl Notifier for Notifier {
            async fn notify(&self, msg: Notification) -> Result<(), NotifyError>;
        }
    }

    #[tokio::test]
    async fn test_multi_notifier_calls_all() {
        let mut mock1 = MockNotifier::new();
        let mut mock2 = MockNotifier::new();

        let notification = Notification {
            id: "test".to_string(),
            title: "Title".to_string(),
            body: "Body".to_string(),
            level: NotifyLevel::Info,
            created_at: Utc::now(),
        };

        let notification_clone = notification.clone();
        mock1
            .expect_notify()
            .with(predicate::function(move |n: &Notification| {
                n.id == notification_clone.id
            }))
            .times(1)
            .returning(|_| Ok(()));

        let notification_clone2 = notification.clone();
        mock2
            .expect_notify()
            .with(predicate::function(move |n: &Notification| {
                n.id == notification_clone2.id
            }))
            .times(1)
            .returning(|_| Ok(()));

        let multi = MultiNotifier::new(vec![Box::new(mock1), Box::new(mock2)]);

        let result = multi.notify(notification).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_multi_notifier_continues_on_failure() {
        let mut mock1 = MockNotifier::new();
        let mut mock2 = MockNotifier::new();

        mock1
            .expect_notify()
            .times(1)
            .returning(|_| Err(NotifyError::SendFailed("fail".to_string())));

        mock2.expect_notify().times(1).returning(|_| Ok(()));

        let multi = MultiNotifier::new(vec![Box::new(mock1), Box::new(mock2)]);

        let notification = Notification {
            id: "test".to_string(),
            title: "Title".to_string(),
            body: "Body".to_string(),
            level: NotifyLevel::Info,
            created_at: Utc::now(),
        };

        let result = multi.notify(notification).await;
        assert!(result.is_ok()); // MultiNotifier swallows errors but logs them
    }
}
