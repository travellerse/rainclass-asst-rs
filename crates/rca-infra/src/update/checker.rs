use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use semver::Version;
use serde::Deserialize;

use crate::update::UpdateError;

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub latest_version: String,
    pub release_url: String,
    pub published_at_unix_ms: i64,
}

#[async_trait]
pub trait UpdateChecker: Send + Sync {
    async fn check_latest(&self, current_version: &str) -> Result<Option<UpdateInfo>, UpdateError>;
}

#[derive(Debug, Clone)]
pub struct GithubReleaseChecker {
    client: reqwest::Client,
    owner: String,
    repo: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseDto {
    tag_name: String,
    html_url: String,
    published_at: String,
    prerelease: bool,
    draft: bool,
}

impl GithubReleaseChecker {
    pub fn new(owner: impl Into<String>, repo: impl Into<String>) -> Result<Self, UpdateError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("RainClassroomAssistant/0.1"),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(UpdateError::Request)?;

        Ok(Self {
            client,
            owner: owner.into(),
            repo: repo.into(),
        })
    }

    fn normalize_version(input: &str) -> Option<Version> {
        let normalized = input.trim_start_matches('v');
        Version::parse(normalized).ok()
    }
}

#[async_trait]
impl UpdateChecker for GithubReleaseChecker {
    async fn check_latest(&self, current_version: &str) -> Result<Option<UpdateInfo>, UpdateError> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/releases/latest",
            self.owner, self.repo
        );

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(UpdateError::Request)?;

        if !response.status().is_success() {
            return Err(UpdateError::UnexpectedStatus(response.status()));
        }

        let release: ReleaseDto = response.json().await.map_err(UpdateError::Request)?;

        if release.draft || release.prerelease {
            return Ok(None);
        }

        let latest = Self::normalize_version(&release.tag_name)
            .ok_or_else(|| UpdateError::InvalidReleaseTag(release.tag_name.clone()))?;
        let current = Self::normalize_version(current_version)
            .ok_or_else(|| UpdateError::InvalidCurrentVersion(current_version.to_string()))?;

        if latest <= current {
            return Ok(None);
        }

        let published_at = DateTime::parse_from_rfc3339(&release.published_at)
            .map_err(UpdateError::InvalidPublishedAt)?
            .with_timezone(&Utc)
            .timestamp_millis();

        Ok(Some(UpdateInfo {
            latest_version: latest.to_string(),
            release_url: release.html_url,
            published_at_unix_ms: published_at,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::GithubReleaseChecker;

    #[test]
    fn normalize_version_accepts_v_prefix() {
        let parsed = GithubReleaseChecker::normalize_version("v1.2.3");
        assert!(parsed.is_some());
    }

    #[test]
    fn normalize_version_rejects_invalid_value() {
        let parsed = GithubReleaseChecker::normalize_version("not-a-semver");
        assert!(parsed.is_none());
    }
}
