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
            (
                AuthState::LoggedIn { user_id },
                AuthEvent::RefreshStarted {
                    user_id: event_user_id,
                },
            ) if *user_id == event_user_id => Ok(AuthState::LoggedIn { user_id: *user_id }),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logged_out_can_start_qr_scan() {
        let state = AuthState::LoggedOut;
        let event = AuthEvent::StartQrScan {
            scene_id: "scene-1".into(),
            token: "token-1".into(),
        };
        let result = state.transition(event);
        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            AuthState::WaitingQrScan { scene_id, token }
            if scene_id == "scene-1" && token == "token-1"
        ));
    }

    #[test]
    fn waiting_qr_scan_can_succeed() {
        let state = AuthState::WaitingQrScan {
            scene_id: "scene-1".into(),
            token: "token-1".into(),
        };
        let event = AuthEvent::LoginSuccess { user_id: 42 };
        let result = state.transition(event);
        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            AuthState::LoggedIn { user_id: 42 }
        ));
    }

    #[test]
    fn waiting_qr_scan_can_fail() {
        let state = AuthState::WaitingQrScan {
            scene_id: "scene-1".into(),
            token: "token-1".into(),
        };
        let event = AuthEvent::Failure {
            reason: "expired".into(),
        };
        let result = state.transition(event);
        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            AuthState::Failed { reason } if reason == "expired"
        ));
    }

    #[test]
    fn logged_in_can_logout() {
        let state = AuthState::LoggedIn { user_id: 42 };
        let event = AuthEvent::Logout;
        let result = state.transition(event);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), AuthState::LoggedOut);
    }

    #[test]
    fn logged_in_can_refresh() {
        let state = AuthState::LoggedIn { user_id: 42 };
        let event = AuthEvent::RefreshSuccess { user_id: 42 };
        let result = state.transition(event);
        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            AuthState::LoggedIn { user_id: 42 }
        ));
    }

    #[test]
    fn logged_in_refresh_with_wrong_user_id_fails() {
        let state = AuthState::LoggedIn { user_id: 42 };
        let event = AuthEvent::RefreshStarted { user_id: 99 };
        let result = state.transition(event);
        assert!(result.is_err());
    }

    #[test]
    fn logged_in_can_fail() {
        let state = AuthState::LoggedIn { user_id: 42 };
        let event = AuthEvent::Failure {
            reason: "network error".into(),
        };
        let result = state.transition(event);
        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            AuthState::Failed { reason } if reason == "network error"
        ));
    }

    #[test]
    fn failed_can_logout() {
        let state = AuthState::Failed {
            reason: "error".into(),
        };
        let event = AuthEvent::Logout;
        let result = state.transition(event);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), AuthState::LoggedOut);
    }

    #[test]
    fn invalid_transitions_return_error() {
        let state = AuthState::LoggedOut;
        let event = AuthEvent::LoginSuccess { user_id: 42 };
        let result = state.transition(event);
        assert!(result.is_err());
    }

    #[test]
    fn can_transition_returns_true_for_valid() {
        let state = AuthState::LoggedOut;
        let event = AuthEvent::StartQrScan {
            scene_id: "scene-1".into(),
            token: "token-1".into(),
        };
        assert!(state.can_transition(&event));
    }

    #[test]
    fn can_transition_returns_false_for_invalid() {
        let state = AuthState::LoggedOut;
        let event = AuthEvent::Logout;
        assert!(!state.can_transition(&event));
    }

    #[test]
    fn user_id_returns_some_when_logged_in() {
        let state = AuthState::LoggedIn { user_id: 42 };
        assert_eq!(state.user_id(), Some(42));
    }

    #[test]
    fn user_id_returns_none_when_not_logged_in() {
        assert_eq!(AuthState::LoggedOut.user_id(), None);
        assert_eq!(
            AuthState::WaitingQrScan {
                scene_id: "s".into(),
                token: "t".into(),
            }
            .user_id(),
            None
        );
        assert_eq!(
            AuthState::Failed {
                reason: "error".into(),
            }
            .user_id(),
            None
        );
    }

    #[test]
    fn is_logged_in_returns_true_only_for_logged_in() {
        assert!(AuthState::LoggedIn { user_id: 42 }.is_logged_in());
        assert!(!AuthState::LoggedOut.is_logged_in());
        assert!(
            !AuthState::WaitingQrScan {
                scene_id: "s".into(),
                token: "t".into(),
            }
            .is_logged_in()
        );
        assert!(
            !AuthState::Failed {
                reason: "error".into(),
            }
            .is_logged_in()
        );
    }

    #[test]
    fn is_waiting_qr_scan_returns_true_only_for_waiting() {
        assert!(
            AuthState::WaitingQrScan {
                scene_id: "s".into(),
                token: "t".into(),
            }
            .is_waiting_qr_scan()
        );
        assert!(!AuthState::LoggedOut.is_waiting_qr_scan());
        assert!(!AuthState::LoggedIn { user_id: 42 }.is_waiting_qr_scan());
        assert!(
            !AuthState::Failed {
                reason: "error".into(),
            }
            .is_waiting_qr_scan()
        );
    }

    #[test]
    fn is_failed_returns_true_only_for_failed() {
        assert!(
            AuthState::Failed {
                reason: "error".into(),
            }
            .is_failed()
        );
        assert!(!AuthState::LoggedOut.is_failed());
        assert!(!AuthState::LoggedIn { user_id: 42 }.is_failed());
        assert!(
            !AuthState::WaitingQrScan {
                scene_id: "s".into(),
                token: "t".into(),
            }
            .is_failed()
        );
    }
}
