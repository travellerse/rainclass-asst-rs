use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::app::AppEvent;
use crate::app::AppNotification;
use crate::app::notify_event_keys;
use crate::monitor::CoreEvent;

use super::AppServiceImpl;

const EVENT_CHANNEL_BUFFER_SIZE: usize = 256;

fn notification_from_event(event: &CoreEvent) -> Option<(&'static str, AppNotification)> {
    match event {
        CoreEvent::AutoAnswerSubmitted {
            lesson_id,
            problem_id,
        } => Some((
            notify_event_keys::AUTO_ANSWER_SUBMITTED,
            AppNotification {
                title: "自动答题".to_string(),
                body: format!(
                    "已成功提交自动答题！(课程 {}, 题目 {})",
                    lesson_id.0.get(),
                    problem_id.0.get()
                ),
            },
        )),
        CoreEvent::AutoCheckinSubmitted {
            lesson_id,
            checkin_id,
        } => Some((
            notify_event_keys::AUTO_CHECKIN_SUBMITTED,
            AppNotification {
                title: "自动签到".to_string(),
                body: format!(
                    "已成功自动签到！(课程 {}, 签到 {})",
                    lesson_id.0.get(),
                    checkin_id.0.get()
                ),
            },
        )),
        CoreEvent::CallPaused {
            lesson_id,
            target_name,
        } => Some((
            notify_event_keys::CALL_PAUSED,
            AppNotification {
                title: "老师正在点名".to_string(),
                body: format!(
                    "老师正在点名：{}！(课程 {})",
                    target_name,
                    lesson_id.0.get()
                ),
            },
        )),
        _ => None,
    }
}

pub(super) struct CoreBackgroundTasks {
    shutdown: Option<oneshot::Sender<()>>,
    join: JoinHandle<()>,
}

impl Drop for CoreBackgroundTasks {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.join.abort();
    }
}

pub(super) async fn start_background_tasks(service: &AppServiceImpl) {
    let mut bg = service.background.lock().await;
    if bg.is_some() {
        return;
    }

    let mut rx = service.deps.monitor_engine.subscribe_events();
    let notifier = service.deps.notifier.clone();
    let inner_clone = Arc::clone(&service.inner);
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

    let (event_tx, mut event_rx) = mpsc::channel::<CoreEvent>(EVENT_CHANNEL_BUFFER_SIZE);

    let join = tokio::spawn(async move {
        let forward_handle = tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                if event_tx.send(event).await.is_err() {
                    break;
                }
            }
        });

        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    forward_handle.abort();
                    break;
                }
                result = event_rx.recv() => {
                    let Some(event) = result else {
                        break;
                    };

                    {
                        let mut guard = inner_clone.lock().await;
                        AppServiceImpl::append_recent_event(&mut guard, event.clone());
                        if let CoreEvent::MonitorStopped { .. } = event {
                            guard.app_state.monitor_running = false;
                            guard.monitor_handle = None;
                        }

                        if let CoreEvent::PresentationUpdated { lesson_id, presentation_id } = &event {
                            let inner_event_clone = Arc::clone(&inner_clone);
                            let lid = *lesson_id;
                            let pid = *presentation_id;
                            tokio::spawn(async move {
                                AppServiceImpl::emit_event_with_inner(
                                    &inner_event_clone,
                                    AppEvent::PresentationDiscovered {
                                        lesson_id: lid,
                                        presentation_id: pid,
                                    },
                                )
                                .await;
                            });
                        }

                        if guard.config.notify_enabled
                            && let Some((event_key, msg)) = notification_from_event(&event)
                            && AppServiceImpl::notify_event_enabled(&guard.config, event_key)
                        {
                            let notifier_clone = notifier.clone();
                            tokio::spawn(async move {
                                let _ = notifier_clone.notify(msg).await;
                            });
                        }
                    }
                    AppServiceImpl::emit_state_changed_with_inner(&inner_clone).await;
                }
            }
        }
    });

    *bg = Some(CoreBackgroundTasks {
        shutdown: Some(shutdown_tx),
        join,
    });
}

pub(super) async fn stop_background_tasks(service: &AppServiceImpl) {
    let mut bg = service.background.lock().await;
    let _ = bg.take();
}
