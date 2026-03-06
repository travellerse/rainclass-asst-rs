#[derive(Debug, Clone)]
pub struct QrLoginBootstrap {
    pub scene_id: String,
    pub token: String,
    pub qr_svg: String,
}

#[derive(Debug, Clone)]
pub enum QrLoginProgress {
    Pending,
    Confirmed(AuthSession),
    Expired,
    Rejected,
}

#[derive(Debug, Clone)]
pub struct AuthSession {
    pub user_id: u64,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_unix_ms: Option<i64>,
}
