use std::sync::Arc;

use slint::Weak;
use tokio::time::{Duration, Instant, sleep};

use crate::app_controller::AppController;
use rca_core::app::{AppQueryResult, AppService};

fn should_refresh_ui_on_event() -> bool {
    true
}

/// Spawn a background loop that listens to application events and triggers UI
/// state sync whenever a new event arrives.
pub fn start_event_loop(controller: Arc<AppController>, ui_handle: Weak<crate::AppWindow>) {
    let mut rx = controller.app.subscribe_events();
    let ctrl = controller.clone();
    controller.spawn_task(async move {
        let mut dirty = false;
        let mut last_change = Instant::now();
        loop {
            tokio::select! {
                maybe = rx.recv() => {
                    let Some(_e) = maybe else { break; };
                    if !should_refresh_ui_on_event() {
                        continue;
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

                    let state = match ctrl.get_state().await {
                        Ok(AppQueryResult::State(state)) => Ok(state),
                        Ok(_) => Err("状态查询返回类型异常".to_string()),
                        Err(e) => Err(format!("状态查询失败: {e}")),
                    };
                    let ui_h = ui_handle.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_h.upgrade() {
                            match state {
                                Ok(state) => crate::ui_helpers::apply_state_to_ui(&ui, &state),
                                Err(err) => ui.set_last_error_text(err.into()),
                            }
                        }
                    });
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
