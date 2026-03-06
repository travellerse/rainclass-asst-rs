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
