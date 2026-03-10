use crate::auth::AuthState;

#[derive(Debug, Clone)]
pub enum AppCommand {
    LoadConfig,
    RestoreSession,
    RefreshSession,
    LoginByQr,
    PollLogin {
        scene_id: String,
    },
    WaitLogin {
        scene_id: String,
        timeout_secs: u64,
    },
    Logout,
    StartMonitor,
    StopMonitor,
    CheckUpdate,
    SaveConfig {
        config: AppConfigDto,
    },
    DownloadPresentation {
        presentation_id: u64,
        lesson_id: Option<u64>,
        save_dir: std::path::PathBuf,
    },
}

#[derive(Debug, Clone)]
pub struct AppConfigDto {
    pub monitor_interval_secs: u64,
    pub auto_checkin_enabled: bool,
    pub auto_answer_enabled: bool,
    pub auto_answer_random_guess: bool,
    pub auto_danmu_enabled: bool,
    pub danmu_threshold: usize,
    pub answer_delay_ms: u64,
    pub answer_delay_type: u32,
    pub answer_delay_custom_percent: u32,
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
            auto_answer_random_guess: false,
            auto_danmu_enabled: true,
            danmu_threshold: 4,
            answer_delay_ms: 500,
            answer_delay_type: 1,
            answer_delay_custom_percent: 50,
            notify_enabled: true,
            webhook_url: String::new(),
            check_update_on_startup: true,
            tenant: "Hetang".to_string(),
            auth_state_hint: None,
        }
    }
}
