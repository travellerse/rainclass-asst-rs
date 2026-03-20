use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

slint::include_modules!();

mod app_controller;
mod command_runner;
mod event_subscriber;
mod ui_binding;
mod ui_helpers;

use app_controller::AndroidController;
use ui_binding::bind_ui;

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: slint::android::AndroidApp) {
    if let Err(err) = run_android_app(app) {
        tracing::error!(error = %err, "android app failed to start");
    }
}

#[cfg(target_os = "android")]
fn run_android_app(app: slint::android::AndroidApp) -> Result<(), Box<dyn Error>> {
    let storage_root = app
        .internal_data_path()
        .or_else(|| app.external_data_path());
    let _log_guards = rca_app::init_default_logger("info");
    slint::android::init(app).map_err(|err| -> Box<dyn Error> { Box::new(err) })?;
    run_window(storage_root)
}

#[cfg(not(target_os = "android"))]
pub fn run_app_on_host() -> Result<(), Box<dyn Error>> {
    let _log_guards = rca_app::init_default_logger("info");
    run_window(None)
}

fn run_window(storage_root: Option<PathBuf>) -> Result<(), Box<dyn Error>> {
    let controller = Arc::new(AndroidController::bootstrap(storage_root)?);
    let ui = AppWindow::new()?;

    bind_ui(&ui, controller.clone());
    event_subscriber::start_event_loop(controller.clone(), ui.as_weak());

    ui.run()?;
    controller.shutdown();
    Ok(())
}
