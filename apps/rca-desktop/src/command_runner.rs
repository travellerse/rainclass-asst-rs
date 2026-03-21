use std::sync::Arc;

use rca_core::app::{AppCommand, AppQueryResult, AppService};

use crate::app_controller::DesktopController;
use crate::ui::UiBindings;
use slint::Weak;

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
        let ui = UiBindings::new(ui_handle);
        let ctrl = self.controller.clone();
        self.controller.spawn_task(async move {
            if let Err(e) = ctrl.app.handle_command(cmd).await {
                ui.show_error(e.to_string());
            }
        });
    }

    pub fn refresh_config(&self, ui_handle: Weak<crate::AppWindow>) {
        let ui = UiBindings::new(ui_handle);
        let ctrl = self.controller.clone();
        self.controller.spawn_task(async move {
            let config = match ctrl.get_config().await {
                Ok(AppQueryResult::Config(config)) => Ok(config),
                Ok(_) => Err("配置查询返回类型异常".to_string()),
                Err(e) => Err(format!("配置查询失败: {e}")),
            };

            match config {
                Ok(config) => ui.render_config(config),
                Err(err) => ui.show_error(err),
            }
        });
    }

    pub fn show_error(&self, ui_handle: Weak<crate::AppWindow>, message: String) {
        UiBindings::new(ui_handle).show_error(message);
    }
}
