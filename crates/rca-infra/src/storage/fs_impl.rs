use std::path::{Path, PathBuf};

use async_trait::async_trait;
use directories::ProjectDirs;
use tokio::fs;

use crate::storage::{AppConfig, ConfigRepository, SessionRecord, SessionRepository, StorageError};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_file: PathBuf,
    pub session_file: PathBuf,
}

impl AppPaths {
    pub fn detect() -> Result<Self, StorageError> {
        let project_dirs = ProjectDirs::from("io", "travellerse", "RainClassroomAssistant")
            .ok_or_else(|| {
                StorageError::InvalidConfig("cannot resolve project directories".to_string())
            })?;

        let config_dir = project_dirs.config_dir().to_path_buf();
        Ok(Self {
            config_file: config_dir.join("config.json"),
            session_file: config_dir.join("session.json"),
        })
    }
}

#[derive(Debug, Clone)]
pub struct JsonFileConfigRepository {
    file_path: PathBuf,
}

impl JsonFileConfigRepository {
    pub fn new(file_path: impl Into<PathBuf>) -> Self {
        Self {
            file_path: file_path.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct JsonFileSessionRepository {
    file_path: PathBuf,
}

impl JsonFileSessionRepository {
    pub fn new(file_path: impl Into<PathBuf>) -> Self {
        Self {
            file_path: file_path.into(),
        }
    }
}

async fn ensure_parent_dir_exists(path: &Path) -> Result<(), StorageError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    Ok(())
}

async fn write_json_atomic(path: &Path, content: &str) -> Result<(), StorageError> {
    ensure_parent_dir_exists(path).await?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content).await?;
    fs::rename(tmp, path).await?;
    Ok(())
}

#[async_trait]
impl ConfigRepository for JsonFileConfigRepository {
    async fn load(&self) -> Result<AppConfig, StorageError> {
        match fs::read_to_string(&self.file_path).await {
            Ok(raw) => {
                let config = serde_json::from_str::<AppConfig>(&raw)?;
                Ok(config)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let default = AppConfig::default();
                self.save(&default).await?;
                Ok(default)
            }
            Err(err) => Err(StorageError::Io(err)),
        }
    }

    async fn save(&self, config: &AppConfig) -> Result<(), StorageError> {
        let content = serde_json::to_string_pretty(config)?;
        write_json_atomic(&self.file_path, &content).await
    }
}

#[async_trait]
impl SessionRepository for JsonFileSessionRepository {
    async fn load(&self) -> Result<Option<SessionRecord>, StorageError> {
        match fs::read_to_string(&self.file_path).await {
            Ok(raw) => Ok(Some(serde_json::from_str::<SessionRecord>(&raw)?)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(StorageError::Io(err)),
        }
    }

    async fn save(&self, session: &SessionRecord) -> Result<(), StorageError> {
        let content = serde_json::to_string_pretty(session)?;
        write_json_atomic(&self.file_path, &content).await
    }

    async fn clear(&self) -> Result<(), StorageError> {
        match fs::remove_file(&self.file_path).await {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(StorageError::Io(err)),
        }
    }
}
