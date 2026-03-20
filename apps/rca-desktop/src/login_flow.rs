use slint::Weak;
use tokio::time::{Duration, sleep};

use crate::command_runner::CommandRunner;
use rca_core::app::{AppCommand, AppQueryResult, AppService};
use rca_core::auth::AuthState;

/// Handle the login flow: start QR, wait for confirmation, refresh UI at each
/// stage.  Runs in a background task (tokio or thread).
pub async fn perform_login(runner: CommandRunner, ui_handle: Weak<crate::AppWindow>) {
    // Step 1: initiate login
    if let Err(e) = runner.controller().login_by_qr().await {
        runner.show_error(ui_handle.clone(), format!("发起登录失败: {e}"));
        return;
    }

    // Step 2: retrieve scene_id from state
    let scene_id = match runner.controller().get_state().await {
        Ok(AppQueryResult::State(state)) => match state.auth_state {
            AuthState::WaitingQrScan { scene_id, .. } => scene_id,
            AuthState::WaitingConfirm { scene_id } => scene_id,
            _ => {
                runner.show_error(ui_handle.clone(), "发起登录后未获取到 scene_id".to_string());
                return;
            }
        },
        _ => return,
    };

    // refresh UI while waiting
    runner.refresh_state(ui_handle.clone());

    // Step 3: wait for login result
    let controller = runner.controller();
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
    runner.refresh_state(ui_handle.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppWindow;
    use crate::app_controller::DesktopController;
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
        let runner = CommandRunner::new(controller.clone());
        // create a temporary UI window to obtain a Weak handle
        let ui = AppWindow::new().unwrap();
        let ui_handle = ui.as_weak();
        let runtime = controller.runtime.clone();
        runtime.block_on(perform_login(runner, ui_handle));
    }
}
