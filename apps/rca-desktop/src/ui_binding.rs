use std::sync::Arc;

use rca_core::app::{AppCommand, AppQueryResult};
use slint::ComponentHandle;

use crate::app_controller::DesktopController;
use crate::command_runner::CommandRunner;
use crate::config_mapping::{ConfigFromUi, build_config_dto_from_ui_values};
use crate::ui::UiBindings;

pub fn bind_ui(ui: &crate::AppWindow, controller: Arc<DesktopController>) {
    let runner = CommandRunner::new(controller.clone());
    let ui_bindings = UiBindings::new(ui.as_weak());

    runner.refresh_config(ui_bindings.weak_handle());

    controller.spawn_task({
        let controller = controller.clone();
        async move {
            if let Ok(AppQueryResult::Config(config)) = controller.get_config().await
                && config.check_update_on_startup
            {
                let _ = controller.check_update().await;
            }
        }
    });

    ui.on_login_clicked({
        let runner = runner.clone();
        let ui_handle = ui_bindings.weak_handle();
        move || {
            let ctrl = runner.controller();
            ctrl.spawn_task(crate::login_flow::perform_login(
                runner.clone(),
                ui_handle.clone(),
            ));
        }
    });

    ui.on_logout_clicked({
        let runner = runner.clone();
        let ui_handle = ui_bindings.weak_handle();
        move || runner.spawn_command(ui_handle.clone(), AppCommand::Logout)
    });

    ui.on_start_monitor_clicked({
        let runner = runner.clone();
        let ui_handle = ui_bindings.weak_handle();
        move || runner.spawn_command(ui_handle.clone(), AppCommand::StartMonitor)
    });

    ui.on_stop_monitor_clicked({
        let runner = runner.clone();
        let ui_handle = ui_bindings.weak_handle();
        move || runner.spawn_command(ui_handle.clone(), AppCommand::StopMonitor)
    });

    ui.on_check_update_clicked({
        let runner = runner.clone();
        let ui_handle = ui_bindings.weak_handle();
        move || runner.spawn_command(ui_handle.clone(), AppCommand::CheckUpdate)
    });

    ui.on_save_config_clicked({
        let runner = runner.clone();
        let ui_handle = ui_bindings.weak_handle();
        move || {
            let ui = match ui_handle.upgrade() {
                Some(ui) => ui,
                None => return,
            };

            let values = ConfigFromUi {
                monitor_interval: ui.get_setting_monitor_interval(),
                auto_checkin: ui.get_setting_auto_checkin(),
                auto_answer: ui.get_setting_auto_answer(),
                auto_answer_random_guess: ui.get_setting_auto_answer_random_guess(),
                answer_delay: ui.get_setting_answer_delay(),
                notify_enabled: ui.get_setting_notify_enabled(),
                webhook_url: ui.get_setting_webhook_url().to_string(),
                check_update_on_startup: ui.get_setting_check_update_on_startup(),
                active_tenant: ui.get_setting_active_tenant().to_string(),
            };

            let config = build_config_dto_from_ui_values(values);
            runner.spawn_command(ui_handle.clone(), AppCommand::SaveConfig { config });
        }
    });
}
