use std::sync::Arc;

use rca_core::app::{AppCommand, AppQueryResult};
use slint::ComponentHandle;

use crate::app_controller::AndroidController;
use crate::command_runner::CommandRunner;

pub fn bind_ui(ui: &crate::AppWindow, controller: Arc<AndroidController>) {
    let runner = CommandRunner::new(controller.clone());
    let ui_handle = ui.as_weak();

    runner.refresh_state(ui_handle.clone());

    let bootstrap_runner = runner.clone();
    controller.spawn_task({
        let controller = controller.clone();
        let ui_handle = ui_handle.clone();
        async move {
            if let Ok(AppQueryResult::State(state)) = controller.get_state().await {
                bootstrap_runner.apply_state(ui_handle, state);
            }
        }
    });

    ui.on_login_clicked({
        let runner = runner.clone();
        let ui_handle = ui_handle.clone();
        move || runner.spawn_command(ui_handle.clone(), AppCommand::LoginByQr)
    });

    ui.on_logout_clicked({
        let runner = runner.clone();
        let ui_handle = ui_handle.clone();
        move || runner.spawn_command(ui_handle.clone(), AppCommand::Logout)
    });

    ui.on_start_monitor_clicked({
        let runner = runner.clone();
        let ui_handle = ui_handle.clone();
        move || runner.spawn_command(ui_handle.clone(), AppCommand::StartMonitor)
    });

    ui.on_stop_monitor_clicked({
        let runner = runner.clone();
        let ui_handle = ui_handle.clone();
        move || runner.spawn_command(ui_handle.clone(), AppCommand::StopMonitor)
    });

    ui.on_check_update_clicked({
        let runner = runner.clone();
        let ui_handle = ui_handle.clone();
        move || runner.spawn_command(ui_handle.clone(), AppCommand::CheckUpdate)
    });
}
