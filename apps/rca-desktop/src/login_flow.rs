use std::sync::Arc;

use slint::Weak;
use tokio::time::{Duration, sleep};

use crate::app_controller::DesktopController;
use crate::ui_helpers::apply_state_to_ui;
use rca_core::app::{AppCommand, AppQueryResult, AppService};
use rca_core::auth::AuthState;

/// Handle the login flow: start QR, wait for confirmation, refresh UI at each
/// stage.  Runs in a background task (tokio or thread).
pub async fn perform_login(controller: Arc<DesktopController>, ui_handle: Weak<crate::AppWindow>) {
    // Step 1: initiate login
    if let Err(e) = controller.login_by_qr().await {
        let ui_h = ui_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                ui.set_last_error_text(format!("发起登录失败: {e}").into());
            }
        });
        return;
    }

    // Step 2: retrieve scene_id from state
    let scene_id = match controller.get_state().await {
        Ok(AppQueryResult::State(state)) => match state.auth_state {
            AuthState::WaitingQrScan { scene_id, .. } => scene_id,
            AuthState::WaitingConfirm { scene_id } => scene_id,
            _ => {
                let ui_h = ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_h.upgrade() {
                        ui.set_last_error_text("发起登录后未获取到 scene_id".into());
                    }
                });
                return;
            }
        },
        _ => return,
    };

    // refresh UI while waiting
    if let Ok(AppQueryResult::State(state)) = controller.get_state().await {
        let ui_h = ui_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                apply_state_to_ui(&ui, &state);
            }
        });
    }

    // Step 3: wait for login result
    let wait = controller.app.handle_command(AppCommand::AwaitLogin {
        scene_id,
        timeout_secs: 20,
    });
    tokio::select! {
        _ = wait => {}
        _ = async {
            loop {
                if ui_handle.upgrade().is_none() {
                    break;
                }
                sleep(Duration::from_millis(300)).await;
            }
        } => { return; }
    }

    // final sync
    if let Ok(AppQueryResult::State(state)) = controller.get_state().await {
        let ui_h = ui_handle.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_h.upgrade() {
                apply_state_to_ui(&ui, &state);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppWindow;
    use slint::ComponentHandle;
    use std::sync::Arc;

    #[test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "EventLoop must be created on the main thread on macOS"
    )]
    #[cfg_attr(target_os = "linux", ignore)]
    fn login_flow_no_panic() {
        // bootstrap controller (real initialization but won't perform network in test)
        let controller = Arc::new(DesktopController::bootstrap().unwrap());
        // create a temporary UI window to obtain a Weak handle
        let ui = AppWindow::new().unwrap();
        let ui_handle = ui.as_weak();
        let runtime = controller.runtime.clone();
        runtime.block_on(perform_login(controller.clone(), ui_handle));
    }
}
