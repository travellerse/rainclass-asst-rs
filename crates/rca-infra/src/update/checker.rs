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
    base_url: String,
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
            base_url: "https://api.github.com".to_string(),
        })
    }

    #[cfg(test)]
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
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
            "{}/repos/{}/{}/releases/latest",
            self.base_url, self.owner, self.repo
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
    use super::{GithubReleaseChecker, UpdateChecker};

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

    #[tokio::test]
    async fn test_github_release_checker_scenarios() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let checker = GithubReleaseChecker::new("owner", "repo")
            .unwrap()
            .with_base_url(url);

        // Case 1: Update available
        let _m1 = server
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "tag_name": "v1.1.0",
                "html_url": "https://github.com/owner/repo/releases/tag/v1.1.0",
                "published_at": "2023-01-01T00:00:00Z",
                "draft": false,
                "prerelease": false
            }"#,
            )
            .create_async()
            .await;

        let result = checker.check_latest("v1.0.0").await.unwrap();
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.latest_version, "1.1.0");

        // Case 2: No update (same version)
        let result = checker.check_latest("v1.1.0").await.unwrap();
        assert!(result.is_none());

        // Case 3: Draft/Prerelease
        server.reset();
        let _m2 = server
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(200)
            .with_body(
                r#"{
                "tag_name": "v1.2.0",
                "html_url": "...",
                "published_at": "2023-01-01T00:00:00Z",
                "draft": true,
                "prerelease": false
            }"#,
            )
            .create_async()
            .await;
        let result = checker.check_latest("v1.0.0").await.unwrap();
        assert!(result.is_none());
    }
}
