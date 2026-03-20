use rca_core::app::AppState;
use rca_core::auth::AuthState;

pub fn auth_status_text(state: &AuthState) -> String {
    match state {
        AuthState::LoggedOut => "未登录".to_string(),
        AuthState::WaitingQrScan { .. } => "等待扫码".to_string(),
        AuthState::WaitingConfirm { .. } => "等待确认".to_string(),
        AuthState::LoggedIn { user_id } => format!("已登录（{user_id}）"),
        AuthState::Refreshing { user_id } => format!("刷新中（{user_id}）"),
        AuthState::Failed { reason } => format!("失败：{reason}"),
    }
}

pub fn auth_status_kind(state: &AuthState) -> &'static str {
    match state {
        AuthState::LoggedOut => "offline",
        AuthState::WaitingQrScan { .. } | AuthState::WaitingConfirm { .. } => "waiting",
        AuthState::LoggedIn { .. } | AuthState::Refreshing { .. } => "online",
        AuthState::Failed { .. } => "error",
    }
}

pub fn apply_state_to_ui(ui: &crate::AppWindow, state: &AppState) {
    ui.set_auth_status_text(auth_status_text(&state.auth_state).into());
    ui.set_auth_status_kind(auth_status_kind(&state.auth_state).into());
    ui.set_monitor_running(state.monitor_running);
    ui.set_monitor_status_text(
        if state.monitor_running {
            "运行中"
        } else {
            "未启动"
        }
        .into(),
    );
    ui.set_last_error_text(state.last_error.clone().unwrap_or_default().into());
    ui.set_lesson_count(state.current_lessons.len() as i32);
    ui.set_recent_event_count(state.recent_events.len() as i32);
}
