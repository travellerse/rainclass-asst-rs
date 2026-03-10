use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use tracing::{error, info};

use crate::notify::{Notification, Notifier, NotifyError};

pub struct WebhookNotifier {
    client: Client,
    webhook_url: String,
}

impl WebhookNotifier {
    pub fn new(webhook_url: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            webhook_url: webhook_url.into(),
        }
    }
}

#[async_trait]
impl Notifier for WebhookNotifier {
    async fn notify(&self, notification: Notification) -> Result<(), NotifyError> {
        let payload = json!({
            "title": notification.title,
            "text": notification.body,
            "desp": notification.body, // Server酱 compatibility
            "body": notification.body,
            "level": format!("{:?}", notification.level),
        });

        match self
            .client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    info!("Webhook push success: {}", self.webhook_url);
                    Ok(())
                } else {
                    let err_text = resp.text().await.unwrap_or_default();
                    error!("Webhook push failed with {}: {}", status, err_text);
                    Err(NotifyError::SendFailed(format!(
                        "Webhook returned status code {}",
                        status
                    )))
                }
            }
            Err(e) => {
                error!("Webhook request failed: {}", e);
                Err(NotifyError::SendFailed(e.to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::NotifyLevel;
    use chrono::Utc;

    #[tokio::test]
    async fn test_webhook_notifier_success() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let _m = server
            .mock("POST", "/")
            .with_status(200)
            .create_async()
            .await;

        let notifier = WebhookNotifier::new(url);
        let notification = Notification {
            id: "test-1".to_string(),
            title: "Test Title".to_string(),
            body: "Test Body".to_string(),
            level: NotifyLevel::Info,
            created_at: Utc::now(),
        };

        let result = notifier.notify(notification).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_webhook_notifier_failure() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let _m = server
            .mock("POST", "/")
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let notifier = WebhookNotifier::new(url);
        let notification = Notification {
            id: "test-2".to_string(),
            title: "Test Title".to_string(),
            body: "Test Body".to_string(),
            level: NotifyLevel::Error,
            created_at: Utc::now(),
        };

        let result = notifier.notify(notification).await;
        assert!(result.is_err());
        match result.err().unwrap() {
            NotifyError::SendFailed(msg) => {
                assert!(msg.contains("500"));
            }
            NotifyError::BackendUnavailable(msg) => panic!("Unexpected error: {}", msg),
            NotifyError::Platform(msg) => panic!("Unexpected error: {}", msg),
        }
    }

    #[tokio::test]
    async fn test_webhook_notifier_network_error() {
        // Use an invalid port to simulate network error
        let notifier = WebhookNotifier::new("http://127.0.0.1:1");
        let notification = Notification {
            id: "test-3".to_string(),
            title: "Test Title".to_string(),
            body: "Test Body".to_string(),
            level: NotifyLevel::Info,
            created_at: Utc::now(),
        };

        let result = notifier.notify(notification).await;
        assert!(result.is_err());
    }
}
