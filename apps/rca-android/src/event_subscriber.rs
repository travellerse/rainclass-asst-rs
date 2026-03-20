use std::sync::Arc;

use slint::Weak;
use tokio::time::{Duration, Instant, sleep};

use crate::app_controller::AndroidController;
use crate::command_runner::CommandRunner;
use rca_core::app::{AppEvent, AppService};

fn should_refresh_ui_on_event() -> bool {
    true
}

pub fn start_event_loop(controller: Arc<AndroidController>, ui_handle: Weak<crate::AppWindow>) {
    let mut rx = controller.app.subscribe_events();
    let runner = CommandRunner::new(controller.clone());
    controller.spawn_task(async move {
        let mut dirty = false;
        let mut last_change = Instant::now();
        let mut latest_state: Option<rca_core::app::AppState> = None;
        loop {
            tokio::select! {
                maybe = rx.recv() => {
                    let Some(event) = maybe else { break; };
                    if !should_refresh_ui_on_event() {
                        continue;
                    }

                    if let AppEvent::StateChanged(state) = event {
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
