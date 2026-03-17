use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use tracing::{error, info};
use url::Url;

use crate::notify::{Notification, Notifier, NotifyError};
use crate::storage::ConfigRepository;

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
                    info!("Webhook push success");
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

pub struct ConfigWebhookNotifier {
    client: Client,
    config_repo: Arc<dyn ConfigRepository>,
}

impl ConfigWebhookNotifier {
    pub fn new(config_repo: Arc<dyn ConfigRepository>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            config_repo,
        }
    }

    fn parse_urls(input: &str) -> Vec<String> {
        input
            .split(|c: char| {
                c == ',' || c == ';' || c == '\n' || c == '\r' || c == '\t' || c == ' '
            })
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    fn safe_url_label(raw: &str) -> String {
        let Ok(url) = Url::parse(raw) else {
            return "invalid-url".to_string();
        };
        match (url.scheme(), url.host_str(), url.port()) {
            (scheme, Some(host), Some(port)) => format!("{scheme}://{host}:{port}"),
            (scheme, Some(host), None) => format!("{scheme}://{host}"),
            (scheme, None, _) => scheme.to_string(),
        }
    }

    async fn send_one(
        &self,
        webhook_url: &str,
        notification: &Notification,
    ) -> Result<(), NotifyError> {
        let payload = json!({
            "title": notification.title,
            "text": notification.body,
            "desp": notification.body,
            "body": notification.body,
            "level": format!("{:?}", notification.level),
        });

        match self.client.post(webhook_url).json(&payload).send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    info!(target: "rca.notify", url = %Self::safe_url_label(webhook_url), "Webhook push success");
                    Ok(())
                } else {
                    let err_text = resp.text().await.unwrap_or_default();
                    error!(
                        target: "rca.notify",
                        url = %Self::safe_url_label(webhook_url),
                        "Webhook push failed with {}: {}",
                        status,
                        err_text
                    );
                    Err(NotifyError::SendFailed(format!(
                        "Webhook returned status code {}",
                        status
                    )))
                }
            }
            Err(e) => {
                error!(
                    target: "rca.notify",
                    url = %Self::safe_url_label(webhook_url),
                    "Webhook request failed: {}",
                    e
                );
                Err(NotifyError::SendFailed(e.to_string()))
            }
        }
    }
}

#[async_trait]
impl Notifier for ConfigWebhookNotifier {
    async fn notify(&self, notification: Notification) -> Result<(), NotifyError> {
        let cfg = self
            .config_repo
            .load()
            .await
            .map_err(|e| NotifyError::BackendUnavailable(e.to_string()))?;

        let urls = Self::parse_urls(&cfg.webhook_url);
        if urls.is_empty() {
            return Ok(());
        }

        let mut success = 0usize;
        let mut last_err: Option<NotifyError> = None;

        for url in urls {
            match self.send_one(&url, &notification).await {
                Ok(()) => success += 1,
                Err(e) => last_err = Some(e),
            }
        }

        if success > 0 {
            Ok(())
        } else {
            Err(last_err
                .unwrap_or_else(|| NotifyError::SendFailed("no webhook succeeded".to_string())))
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
