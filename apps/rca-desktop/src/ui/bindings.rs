use rca_core::app::{AppConfigDto, AppState};
use slint::Weak;

#[derive(Clone)]
pub struct UiBindings {
    ui: Weak<crate::AppWindow>,
}

impl UiBindings {
    pub fn new(ui: Weak<crate::AppWindow>) -> Self {
        Self { ui }
    }

    pub fn weak_handle(&self) -> Weak<crate::AppWindow> {
        self.ui.clone()
    }

    pub fn render_state(&self, state: AppState) {
        let ui_handle = self.ui.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                crate::ui::render::render_state(&ui, &state);
            }
        });
    }

    pub fn render_config(&self, config: AppConfigDto) {
        let ui_handle = self.ui.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                crate::ui::render::render_config(&ui, &config);
            }
        });
    }

    pub fn show_error(&self, message: String) {
        let ui_handle = self.ui.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                ui.set_last_error_text(message.into());
            }
        });
    }
}
