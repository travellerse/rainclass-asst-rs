use std::sync::Arc;

use slint::Weak;

use crate::app_controller::AppController;
use rca_core::app::AppService;

fn should_refresh_ui_on_event() -> bool {
    true
}

/// Spawn a background loop that listens to application events and triggers UI
/// state sync whenever a new event arrives.
pub fn start_event_loop(controller: Arc<AppController>, ui_handle: Weak<crate::AppWindow>) {
    let mut rx = controller.app.subscribe_events();
    let runtime = controller.runtime.clone();
    std::thread::spawn(move || {
        while let Some(_e) = runtime.block_on(rx.recv()) {
            if !should_refresh_ui_on_event() {
                continue;
            }
            if let Some(ui) = ui_handle.upgrade() {
                // after receiving an event, refresh full UI state
                let _ = runtime.block_on(async {
                    crate::ui_helpers::sync_ui_state(&ui, &controller);
                    Ok::<(), ()>(())
                });
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
