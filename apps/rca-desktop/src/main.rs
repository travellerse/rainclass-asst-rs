#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::error::Error;
use std::sync::Arc;

use rca_core::app::{AppCommand, AppConfigDto, AppQueryResult, AppService};

rust_i18n::i18n!("../../locales", fallback = "zh-CN");

slint::include_modules!();

mod app_controller;
mod event_subscriber;
mod login_flow;
mod ui_helpers;

use app_controller::AppController;
use ui_helpers::{sync_config_to_ui, sync_ui_state};

/// Run an `AppCommand` on the controller in a background thread and refresh the UI.
fn spawn_command(
    controller: Arc<AppController>,
    ui_handle: slint::Weak<AppWindow>,
    cmd: AppCommand,
) {
    let _ = std::thread::spawn(move || {
        if let Err(e) = controller
            .runtime
            .block_on(controller.app.handle_command(cmd))
        {
            let ctrl = controller.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_handle.upgrade() {
                    ui.set_last_error_text(format!("{e}").into());
                    sync_ui_state(&ui, &ctrl);
                }
            });
            return;
        }
        let ctrl = controller.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                sync_ui_state(&ui, &ctrl);
            }
        });
    });
}

fn main() -> Result<(), Box<dyn Error>> {
    let log_dir = match rca_infra::storage::AppPaths::detect() {
        Ok(paths) => paths.log_dir,
        Err(_) => std::env::current_dir().unwrap_or_default().join("logs"),
    };
    let _log_guards = rca_infra::log::init_logger(log_dir, "info");

    // ── Bootstrap controller & UI ──
    let controller = Arc::new(AppController::bootstrap()?);
    let ui = AppWindow::new()?;

    ui.global::<I18n>()
        .on_t(|key| rust_i18n::t!(key.as_str()).to_string().into());

    // optional update check from controller state
    let startup_should_check_update = controller
        .runtime
        .block_on(controller.get_config())
        .ok()
        .and_then(|result| match result {
            AppQueryResult::Config(config) => Some(config.check_update_on_startup),
            _ => None,
        })
        .unwrap_or(false);
    if startup_should_check_update {
        let _ = controller.runtime.block_on(controller.check_update());
    }

    // Initial sync
    sync_ui_state(&ui, &controller);
    sync_config_to_ui(&ui, &controller);

    // ── Bind callbacks ──

    // Login
    ui.on_login_clicked({
        let controller = controller.clone();
        let ui_handle = ui.as_weak();
        move || {
            let controller = controller.clone();
            let ui_handle = ui_handle.clone();
            std::thread::spawn(move || {
                let rt = controller.runtime.clone();
                rt.block_on(async move {
                    login_flow::perform_login(controller, ui_handle).await;
                });
            });
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
            let llm_config = controller
                .runtime
                .block_on(controller.get_config())
                .ok()
                .and_then(|r| {
                    if let AppQueryResult::Config(c) = r {
                        c.llm_config
                    } else {
                        None
                    }
                });

            let config = AppConfigDto {
                monitor_interval_secs: ui.get_setting_monitor_interval().max(1) as u64,
                auto_checkin_enabled: ui.get_setting_auto_checkin(),
                auto_answer_enabled: ui.get_setting_auto_answer(),
                auto_answer_random_guess: ui.get_setting_auto_answer_random_guess(),
                auto_danmu_enabled: true,
                danmu_threshold: 4,
                answer_delay_ms: ui.get_setting_answer_delay().max(0) as u64,
                answer_delay_type: 1,
                answer_delay_custom_percent: 50,
                notify_enabled: ui.get_setting_notify_enabled(),
                webhook_url: ui.get_setting_webhook_url().to_string(),
                check_update_on_startup: ui.get_setting_check_update_on_startup(),
                tenant: ui.get_setting_active_tenant().to_string(),
                auth_state_hint: None,
                llm_config,
            };
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
    Ok(())
}
