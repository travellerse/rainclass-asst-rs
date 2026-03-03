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
