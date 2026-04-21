use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Arc;
use tracing::{error, info};
use url::Url;

use crate::notify::{Notification, Notifier, NotifyError};
use crate::storage::ConfigRepository;

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || is_unique_local_ipv6(v6)
                || is_link_local_ipv6(v6)
        }
    }
}

fn is_unique_local_ipv6(ip: &Ipv6Addr) -> bool {
    let segments = ip.segments();
    (segments[0] & 0xfe00) == 0xfc00
}

fn is_link_local_ipv6(ip: &Ipv6Addr) -> bool {
    let segments = ip.segments();
    (segments[0] & 0xffc0) == 0xfe80
}

fn validate_webhook_url(url: &str) -> Result<(), NotifyError> {
    let parsed =
        Url::parse(url).map_err(|e| NotifyError::SendFailed(format!("Invalid URL: {}", e)))?;

    if parsed.scheme() != "https" {
        return Err(NotifyError::InsecureUrl(
            "Webhook URL must use HTTPS".to_string(),
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| NotifyError::SendFailed("URL has no host".to_string()))?;

    if host == "localhost" || host == "127.0.0.1" || host.starts_with("127.") {
        return Err(NotifyError::PrivateNetworkNotAllowed(
            "localhost is not allowed".to_string(),
        ));
    }

    if let Some(ip) = host.parse::<IpAddr>().ok().filter(is_private_ip) {
        return Err(NotifyError::PrivateNetworkNotAllowed(format!(
            "Private IP {} is not allowed",
            ip
        )));
    }

    let lower_host = host.to_lowercase();
    if lower_host.ends_with(".internal")
        || lower_host.ends_with(".local")
        || lower_host.ends_with(".localhost")
        || lower_host == "metadata.google.internal"
        || lower_host.ends_with(".metadata.google.internal")
        || lower_host == "instance-data.ec2.internal"
        || lower_host.ends_with(".ec2.internal")
    {
        return Err(NotifyError::PrivateNetworkNotAllowed(format!(
            "Internal hostname {} is not allowed",
            host
        )));
    }

    Ok(())
}

pub struct WebhookNotifier {
    client: Client,
    webhook_url: String,
}

impl WebhookNotifier {
    pub fn new(webhook_url: impl Into<String>) -> Result<Self, NotifyError> {
        let url: String = webhook_url.into();
        validate_webhook_url(&url)?;

        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            webhook_url: url,
        })
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

    fn parse_and_validate_urls(input: &str) -> Result<Vec<String>, NotifyError> {
        let urls: Vec<String> = input
            .split(|c: char| {
                c == ',' || c == ';' || c == '\n' || c == '\r' || c == '\t' || c == ' '
            })
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();

        for url in &urls {
            validate_webhook_url(url)?;
        }

        Ok(urls)
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

        let urls = Self::parse_and_validate_urls(&cfg.webhook_url)?;
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

    #[tokio::test]
    async fn test_webhook_notifier_rejects_http() {
        let result = WebhookNotifier::new("http://example.com/webhook");
        assert!(result.is_err());
        match result.err().unwrap() {
            NotifyError::InsecureUrl(_) => {}
            _ => panic!("Expected InsecureUrl error"),
        }
    }

    #[tokio::test]
    async fn test_webhook_notifier_rejects_localhost() {
        let result = WebhookNotifier::new("https://localhost:8080/webhook");
        assert!(result.is_err());
        match result.err().unwrap() {
            NotifyError::PrivateNetworkNotAllowed(_) => {}
            _ => panic!("Expected PrivateNetworkNotAllowed error"),
        }
    }

    #[tokio::test]
    async fn test_webhook_notifier_rejects_private_ip() {
        let result = WebhookNotifier::new("https://192.168.1.1/webhook");
        assert!(result.is_err());
        match result.err().unwrap() {
            NotifyError::PrivateNetworkNotAllowed(_) => {}
            _ => panic!("Expected PrivateNetworkNotAllowed error"),
        }
    }

    #[tokio::test]
    async fn test_webhook_notifier_rejects_metadata_endpoint() {
        let result = WebhookNotifier::new("https://metadata.google.internal/computeMetadata/v1/");
        assert!(result.is_err());
        match result.err().unwrap() {
            NotifyError::PrivateNetworkNotAllowed(_) => {}
            _ => panic!("Expected PrivateNetworkNotAllowed error"),
        }
    }
}
