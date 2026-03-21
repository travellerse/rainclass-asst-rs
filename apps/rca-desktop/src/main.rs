#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::error::Error;
use std::sync::Arc;

rust_i18n::i18n!("../../locales", fallback = "zh-CN");

slint::include_modules!();

mod app_controller;
mod command_runner;
mod config_mapping;
mod event_subscriber;
mod login_flow;
mod ui;
mod ui_binding;
mod ui_helpers;

use app_controller::DesktopController;
use ui_binding::bind_ui;

fn main() -> Result<(), Box<dyn Error>> {
    let _log_guards = rca_app::init_default_logger("info");

    // ── Bootstrap controller & UI ──
    let controller = Arc::new(DesktopController::bootstrap()?);
    let ui = AppWindow::new()?;

    ui.global::<I18n>()
        .on_t(|key| rust_i18n::t!(key.as_str()).to_string().into());

    bind_ui(&ui, controller.clone());

    // ── Start background event subscription ──
    event_subscriber::start_event_loop(controller.clone(), ui.as_weak());

    ui.run()?;
    controller.shutdown();
    Ok(())
}
