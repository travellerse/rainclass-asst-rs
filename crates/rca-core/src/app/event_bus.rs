use tokio::sync::mpsc;

use crate::app::AppEvent;

pub type AppEventSender = mpsc::Sender<AppEvent>;
pub type AppEventReceiver = mpsc::Receiver<AppEvent>;
