#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthState {
    LoggedOut,
    WaitingQrScan { scene_id: String, token: String },
    LoggedIn { user_id: u64 },
    Failed { reason: String },
}

/// Events that can trigger state transitions in the auth state machine.
#[derive(Debug, Clone)]
pub enum AuthEvent {
    StartQrScan { scene_id: String, token: String },
    QrScanSuccess { user_id: u64 },
    LoginSuccess { user_id: u64 },
    RefreshStarted { user_id: u64 },
    RefreshSuccess { user_id: u64 },
    RefreshFailed { reason: String },
    Logout,
    Failure { reason: String },
}

impl AuthState {
    /// Attempts to transition to a new state based on the given event.
    /// Returns an error if the transition is not valid.
    pub fn transition(&self, event: AuthEvent) -> Result<Self, crate::auth::AuthError> {
        match (self, event) {
            // From LoggedOut: can start QR scan
            (AuthState::LoggedOut, AuthEvent::StartQrScan { scene_id, token }) => {
                Ok(AuthState::WaitingQrScan { scene_id, token })
            }

            // From WaitingQrScan: can succeed or fail
            (
                AuthState::WaitingQrScan { .. },
                AuthEvent::QrScanSuccess { user_id } | AuthEvent::LoginSuccess { user_id },
            ) => Ok(AuthState::LoggedIn { user_id }),
            (AuthState::WaitingQrScan { .. }, AuthEvent::Failure { reason }) => {
                Ok(AuthState::Failed { reason })
            }

            // From LoggedIn: can logout, refresh, or fail
            (AuthState::LoggedIn { .. }, AuthEvent::Logout) => Ok(AuthState::LoggedOut),
            (AuthState::LoggedIn { user_id }, AuthEvent::RefreshStarted { .. }) => {
                Ok(AuthState::LoggedIn { user_id: *user_id })
            }
            (AuthState::LoggedIn { .. }, AuthEvent::RefreshSuccess { user_id }) => {
                Ok(AuthState::LoggedIn { user_id })
            }
            (AuthState::LoggedIn { .. }, AuthEvent::RefreshFailed { reason }) => {
                Ok(AuthState::Failed { reason })
            }
            (AuthState::LoggedIn { .. }, AuthEvent::Failure { reason }) => {
                Ok(AuthState::Failed { reason })
            }

            // From Failed: can logout to reset
            (AuthState::Failed { .. }, AuthEvent::Logout) => Ok(AuthState::LoggedOut),

            // All other transitions are invalid
            (from, to) => Err(crate::auth::AuthError::InvalidTransition {
                from: format!("{:?}", from),
                to: format!("{:?}", to),
            }),
        }
    }

    /// Returns true if the given event is valid for the current state.
    pub fn can_transition(&self, event: &AuthEvent) -> bool {
        self.transition(event.clone()).is_ok()
    }

    /// Returns the user ID if logged in, None otherwise.
    pub fn user_id(&self) -> Option<u64> {
        match self {
            AuthState::LoggedIn { user_id } => Some(*user_id),
            _ => None,
        }
    }

    /// Returns true if currently logged in.
    pub fn is_logged_in(&self) -> bool {
        matches!(self, AuthState::LoggedIn { .. })
    }

    /// Returns true if waiting for QR scan.
    pub fn is_waiting_qr_scan(&self) -> bool {
        matches!(self, AuthState::WaitingQrScan { .. })
    }

    /// Returns true if in failed state.
    pub fn is_failed(&self) -> bool {
        matches!(self, AuthState::Failed { .. })
    }
}
