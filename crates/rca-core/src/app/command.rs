use crate::auth::AuthState;

#[derive(Debug, Clone)]
pub enum AppCommand {
    LoadConfig,
    RestoreSession,
    RefreshSession,
    LoginByQr,
    PollLogin { scene_id: String },
    WaitLogin { scene_id: String, timeout_secs: u64 },
    Logout,
    StartMonitor,
    StopMonitor,
    CheckUpdate,
    SaveConfig { config: AppConfigDto },
}

#[derive(Debug, Clone)]
pub struct AppConfigDto {
    pub monitor_interval_secs: u64,
    pub auto_checkin_enabled: bool,
    pub auto_answer_enabled: bool,
    pub answer_delay_ms: u64,
    pub notify_enabled: bool,
    pub webhook_url: String,
    pub check_update_on_startup: bool,
    pub tenant: String,
    pub auth_state_hint: Option<AuthState>,
}

impl Default for AppConfigDto {
    fn default() -> Self {
        Self {
            monitor_interval_secs: 5,
            auto_checkin_enabled: true,
            auto_answer_enabled: true,
            answer_delay_ms: 500,
            notify_enabled: true,
            webhook_url: String::new(),
            check_update_on_startup: true,
            tenant: "Hetang".to_string(),
            auth_state_hint: None,
        }
    }
}
