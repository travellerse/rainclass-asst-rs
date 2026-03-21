use rca_core::app::{AppConfigDto, AppState};

pub fn render_state(ui: &crate::AppWindow, state: &AppState) {
    crate::ui_helpers::apply_state_to_ui(ui, state);
}

pub fn render_config(ui: &crate::AppWindow, config: &AppConfigDto) {
    crate::ui_helpers::apply_config_to_ui(ui, config);
}
