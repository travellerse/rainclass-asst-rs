use std::sync::Arc;

use slint::Weak;

use crate::app_controller::AppController;
use rca_core::app::AppService;

/// Spawn a background loop that listens to application events and triggers UI
/// state sync whenever a new event arrives.
pub fn start_event_loop(
    controller: Arc<AppController>,
    ui_handle: Weak<crate::AppWindow>,
) {
    let mut rx = controller.app.subscribe_events();
    let runtime = controller.runtime.clone();
    std::thread::spawn(move || {
        while let Some(_e) = runtime.block_on(rx.recv()) {
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
