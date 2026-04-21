use std::sync::Arc;

use slint::Weak;
use tokio::time::{Duration, Instant, sleep};

use crate::app_controller::DesktopController;
use crate::command_runner::CommandRunner;
use rca_core::app::{AppEvent, AppService};

fn should_refresh_ui_on_event() -> bool {
    true
}

/// Spawn a background loop that listens to application events and triggers UI
/// state sync whenever a new event arrives.
pub fn start_event_loop(controller: Arc<DesktopController>, ui_handle: Weak<crate::AppWindow>) {
    let mut rx = controller
        .runtime
        .block_on(controller.app.subscribe_events());
    let runner = CommandRunner::new(controller.clone());
    controller.spawn_task(async move {
        let mut dirty = false;
        let mut last_change = Instant::now();
        let mut latest_state: Option<rca_core::app::AppState> = None;
        loop {
            tokio::select! {
                maybe = rx.recv() => {
                    let Some(e) = maybe else { break; };
                    if !should_refresh_ui_on_event() {
                        continue;
                    }

                    if let AppEvent::StateChanged(state) = e {
                        latest_state = Some(state);
                    }
                    dirty = true;
                    last_change = Instant::now();
                }
                _ = sleep(Duration::from_millis(60)) => {
                    if ui_handle.upgrade().is_none() {
                        break;
                    }
                    if !dirty || last_change.elapsed() < Duration::from_millis(120) {
                        continue;
                    }
                    dirty = false;

                    // 优先使用事件携带的快照，避免每次刷新都向 core 发起查询。
                    // 若期间未收到 StateChanged（例如未来新增事件类型），再回退到查询。
                    if let Some(state) = latest_state.take() {
                        runner.apply_state(ui_handle.clone(), state);
                    } else {
                        runner.refresh_state(ui_handle.clone());
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_refresh_ui_on_event_is_enabled() {
        assert!(should_refresh_ui_on_event());
    }
}
