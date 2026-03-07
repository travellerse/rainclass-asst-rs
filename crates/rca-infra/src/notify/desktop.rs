use crate::notify::{Notification, Notifier, NotifyError};
use async_trait::async_trait;

pub struct DesktopNotifier;

impl DesktopNotifier {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DesktopNotifier {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Notifier for DesktopNotifier {
    async fn notify(&self, msg: Notification) -> Result<(), NotifyError> {
        let mut notification = notify_rust::Notification::new();
        notification
            .summary(&msg.title)
            .body(&msg.body)
            .appname("RainClassroomAssistant");

        notification
            .show()
            .map(|_| ())
            .map_err(|error| NotifyError::SendFailed(error.to_string()))
    }
}
