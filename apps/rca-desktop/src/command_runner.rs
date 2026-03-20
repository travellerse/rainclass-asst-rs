use std::sync::Arc;

use rca_core::app::AppState;
use rca_core::app::{AppCommand, AppQueryResult, AppService};
use slint::Weak;

use crate::app_controller::DesktopController;

#[derive(Clone)]
pub struct CommandRunner {
    controller: Arc<DesktopController>,
}

impl CommandRunner {
    pub fn new(controller: Arc<DesktopController>) -> Self {
        Self { controller }
    }

    pub fn controller(&self) -> Arc<DesktopController> {
        self.controller.clone()
    }

    pub fn spawn_command(&self, ui_handle: Weak<crate::AppWindow>, cmd: AppCommand) {
        let ctrl = self.controller.clone();
        self.controller.spawn_task(async move {
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
                        Ok(state) => crate::ui_helpers::apply_state_to_ui(&ui, &state),
                        Err(err) => ui.set_last_error_text(err.into()),
                    }
                    if let Some(err) = cmd_err {
                        ui.set_last_error_text(err.into());
                    }
                }
            });
        });
    }

    pub fn apply_state(&self, ui_handle: Weak<crate::AppWindow>, state: AppState) {
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                crate::ui_helpers::apply_state_to_ui(&ui, &state);
            }
        });
    }

    pub fn refresh_state(&self, ui_handle: Weak<crate::AppWindow>) {
        let ctrl = self.controller.clone();
        self.controller.spawn_task(async move {
            let state = match ctrl.get_state().await {
                Ok(AppQueryResult::State(state)) => Ok(state),
                Ok(_) => Err("状态查询返回类型异常".to_string()),
                Err(e) => Err(format!("状态查询失败: {e}")),
            };

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_handle.upgrade() {
                    match state {
                        Ok(state) => crate::ui_helpers::apply_state_to_ui(&ui, &state),
                        Err(err) => ui.set_last_error_text(err.into()),
                    }
                }
            });
        });
    }

    pub fn refresh_config(&self, ui_handle: Weak<crate::AppWindow>) {
        let ctrl = self.controller.clone();
        self.controller.spawn_task(async move {
            let config = match ctrl.get_config().await {
                Ok(AppQueryResult::Config(config)) => Ok(config),
                Ok(_) => Err("配置查询返回类型异常".to_string()),
                Err(e) => Err(format!("配置查询失败: {e}")),
            };

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_handle.upgrade() {
                    match config {
                        Ok(config) => crate::ui_helpers::apply_config_to_ui(&ui, &config),
                        Err(err) => ui.set_last_error_text(err.into()),
                    }
                }
            });
        });
    }

    pub fn show_error(&self, ui_handle: Weak<crate::AppWindow>, message: String) {
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                ui.set_last_error_text(message.into());
            }
        });
    }
}
