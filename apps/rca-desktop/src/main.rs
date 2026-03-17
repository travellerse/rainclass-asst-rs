#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::error::Error;
use std::sync::Arc;

use rca_core::app::{AppCommand, AppQueryResult, AppService};

rust_i18n::i18n!("../../locales", fallback = "zh-CN");

slint::include_modules!();

mod app_controller;
mod config_mapping;
mod event_subscriber;
mod login_flow;
mod ui_helpers;

use app_controller::AppController;
use config_mapping::{ConfigFromUi, build_config_dto_from_ui_values};
use ui_helpers::{apply_state_to_ui, sync_config_to_ui, sync_ui_state};

/// Run an `AppCommand` on the controller in the background and refresh the UI.
fn spawn_command(
    controller: Arc<AppController>,
    ui_handle: slint::Weak<AppWindow>,
    cmd: AppCommand,
) {
    let ctrl = controller.clone();
    controller.spawn_task(async move {
        let cmd_err = ctrl
            .app
            .handle_command(cmd)
            .await
            .err()
            .map(|e| e.to_string());

        let state = match ctrl.get_state().await {
            Ok(AppQueryResult::State(state)) => Ok(state),
            Ok(_) => Err("状态查询返回类型异常".to_string()),
            Err(e) => Err(format!("状态查询失败: {e}")),
        };

        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                match state {
                    Ok(state) => apply_state_to_ui(&ui, &state),
                    Err(err) => ui.set_last_error_text(err.into()),
                }
                if let Some(err) = cmd_err {
                    ui.set_last_error_text(err.into());
                }
            }
        });
    });
}

fn main() -> Result<(), Box<dyn Error>> {
    let _log_guards = rca_app::init_default_logger("info");

    // ── Bootstrap controller & UI ──
    let controller = Arc::new(AppController::bootstrap()?);
    let ui = AppWindow::new()?;

    ui.global::<I18n>()
        .on_t(|key| rust_i18n::t!(key.as_str()).to_string().into());

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

    // Initial sync
    sync_ui_state(&ui, &controller);
    sync_config_to_ui(&ui, &controller);

    // ── Bind callbacks ──

    // Login
    ui.on_login_clicked({
        let controller = controller.clone();
        let ui_handle = ui.as_weak();
        move || {
            let ctrl = controller.clone();
            controller.spawn_task(login_flow::perform_login(ctrl, ui_handle.clone()));
        }
    });

    // Logout
    ui.on_logout_clicked({
        let controller = controller.clone();
        let ui_handle = ui.as_weak();
        move || {
            let ctr = controller.clone();
            let ui_h = ui_handle.clone();
            spawn_command(ctr, ui_h, AppCommand::Logout);
        }
    });

    // Start monitor
    ui.on_start_monitor_clicked({
        let controller = controller.clone();
        let ui_handle = ui.as_weak();
        move || {
            spawn_command(
                controller.clone(),
                ui_handle.clone(),
                AppCommand::StartMonitor,
            );
        }
    });

    // Stop monitor
    ui.on_stop_monitor_clicked({
        let controller = controller.clone();
        let ui_handle = ui.as_weak();
        move || {
            spawn_command(
                controller.clone(),
                ui_handle.clone(),
                AppCommand::StopMonitor,
            );
        }
    });

    // Check update
    ui.on_check_update_clicked({
        let controller = controller.clone();
        let ui_handle = ui.as_weak();
        move || {
            spawn_command(
                controller.clone(),
                ui_handle.clone(),
                AppCommand::CheckUpdate,
            );
        }
    });

    // Save config
    ui.on_save_config_clicked({
        let controller = controller.clone();
        let ui_handle = ui.as_weak();
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
            spawn_command(
                controller.clone(),
                ui_handle.clone(),
                AppCommand::SaveConfig { config },
            );
        }
    });

    // ── Start background event subscription ──
    event_subscriber::start_event_loop(controller.clone(), ui.as_weak());

    ui.run()?;
    controller.shutdown();
    Ok(())
}
