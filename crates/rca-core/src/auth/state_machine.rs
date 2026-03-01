#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthState {
    LoggedOut,
    WaitingQrScan { scene_id: String, token: String },
    WaitingConfirm { scene_id: String },
    LoggedIn { user_id: u64 },
    Refreshing { user_id: u64 },
    Failed { reason: String },
}
