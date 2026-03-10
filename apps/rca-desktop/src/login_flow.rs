use std::sync::Arc;

use slint::Weak;

use crate::app_controller::AppController;
use crate::ui_helpers::{auth_status_kind, auth_status_text};
use rca_core::app::{AppCommand, AppQueryResult, AppService};
use rca_core::auth::AuthState;

/// Handle the login flow: start QR, wait for confirmation, refresh UI at each
/// stage.  Runs in a background task (tokio or thread).
pub async fn perform_login(controller: Arc<AppController>, ui_handle: Weak<crate::AppWindow>) {
    // Step 1: initiate login
    if let Err(e) = controller.login_by_qr().await {
        if let Some(ui) = ui_handle.upgrade() {
            ui.set_last_error_text(format!("发起登录失败: {e}").into());
        }
        return;
    }

    // Step 2: retrieve scene_id from state
    let scene_id = match controller.get_state().await {
        Ok(AppQueryResult::State(state)) => match state.auth_state {
            AuthState::WaitingQrScan { scene_id, .. } => scene_id,
            AuthState::WaitingConfirm { scene_id } => scene_id,
            _ => {
                if let Some(ui) = ui_handle.upgrade() {
                    ui.set_last_error_text("发起登录后未获取到 scene_id".into());
                }
                return;
            }
        },
        _ => return,
    };

    // refresh UI while waiting
    if let Some(ui) = ui_handle.upgrade()
        && let Ok(AppQueryResult::State(state)) = controller.get_state().await
    {
        ui.set_auth_status_text(auth_status_text(&state.auth_state).into());
        ui.set_auth_status_kind(auth_status_kind(&state.auth_state).into());
    }

    // Step 3: wait for login result
    let _ = controller
        .app
        .handle_command(AppCommand::WaitLogin {
            scene_id,
            timeout_secs: 20,
        })
        .await;

    // final sync
    if let Some(ui) = ui_handle.upgrade()
        && let Ok(AppQueryResult::State(state)) = controller.get_state().await
    {
        ui.set_auth_status_text(auth_status_text(&state.auth_state).into());
        ui.set_auth_status_kind(auth_status_kind(&state.auth_state).into());
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
    fn login_flow_no_panic() {
        // bootstrap controller (real initialization but won't perform network in test)
        let controller = Arc::new(AppController::bootstrap().unwrap());
        // create a temporary UI window to obtain a Weak handle
        let ui = AppWindow::new().unwrap();
        let ui_handle = ui.as_weak();
        let runtime = controller.runtime.clone();
        runtime.block_on(perform_login(controller.clone(), ui_handle));
    }
}
