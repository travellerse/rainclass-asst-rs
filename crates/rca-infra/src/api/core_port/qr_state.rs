use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;

use rca_core::app::ports::ApiPortError;
use rca_core::auth::{AuthSession, QrLoginProgress};

#[derive(Debug, Clone)]
pub(crate) enum QrSceneState {
    Pending,
    Confirmed(AuthSession),
    Expired,
    Rejected,
}

#[derive(Clone)]
pub(crate) struct QrStateStore {
    states: Arc<Mutex<HashMap<String, QrSceneState>>>,
    notify: Arc<Notify>,
}

impl QrStateStore {
    pub(crate) fn new() -> Self {
        Self {
            states: Arc::new(Mutex::new(HashMap::new())),
            notify: Arc::new(Notify::new()),
        }
    }

    pub(crate) fn set(&self, scene_id: &str, state: QrSceneState) {
        let mut guard = self.states.lock().expect("qr state poisoned");
        guard.insert(scene_id.to_string(), state);
        self.notify.notify_waiters();
    }

    pub(crate) fn poll(&self, scene_id: &str) -> Result<QrLoginProgress, ApiPortError> {
        let mut guard = self.states.lock().expect("qr state poisoned");
        let Some(state) = guard.get(scene_id).cloned() else {
            return Err(ApiPortError::protocol(format!(
                "unknown scene_id: {scene_id}"
            )));
        };

        match state {
            QrSceneState::Pending => Ok(QrLoginProgress::Pending),
            QrSceneState::Expired => {
                guard.remove(scene_id);
                Ok(QrLoginProgress::Expired)
            }
            QrSceneState::Rejected => {
                guard.remove(scene_id);
                Ok(QrLoginProgress::Rejected)
            }
            QrSceneState::Confirmed(session) => {
                guard.remove(scene_id);
                Ok(QrLoginProgress::Confirmed(session))
            }
        }
    }

    pub(crate) async fn wait(
        &self,
        scene_id: &str,
        timeout_secs: u64,
    ) -> Result<QrLoginProgress, ApiPortError> {
        let timeout_secs = timeout_secs.max(1);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

        loop {
            let maybe_progress = {
                let mut guard = self.states.lock().expect("qr state poisoned");
                let Some(state) = guard.get(scene_id).cloned() else {
                    return Err(ApiPortError::protocol(format!(
                        "unknown scene_id: {scene_id}"
                    )));
                };

                match state {
                    QrSceneState::Pending => None,
                    QrSceneState::Expired => {
                        guard.remove(scene_id);
                        Some(QrLoginProgress::Expired)
                    }
                    QrSceneState::Rejected => {
                        guard.remove(scene_id);
                        Some(QrLoginProgress::Rejected)
                    }
                    QrSceneState::Confirmed(session) => {
                        guard.remove(scene_id);
                        Some(QrLoginProgress::Confirmed(session))
                    }
                }
            };

            if let Some(progress) = maybe_progress {
                return Ok(progress);
            }

            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Ok(QrLoginProgress::Pending);
            }

            let wait_for = deadline.saturating_duration_since(now);
            if tokio::time::timeout(wait_for, self.notify.notified())
                .await
                .is_err()
            {
                return Ok(QrLoginProgress::Pending);
            }
        }
    }
}
