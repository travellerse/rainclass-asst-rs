use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("websocket connect error: {0}")]
    WebSocketConnect(String),

    #[error("websocket request error: {0}")]
    WebSocketRequest(String),

    #[error("websocket send error: {0}")]
    WebSocketSend(String),

    #[error("websocket receive error: {0}")]
    WebSocketReceive(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("unauthorized")]
    Unauthorized,

    #[error("rate limited")]
    RateLimited,

    #[error("lesson ended")]
    LessonEnded,

    #[error("invalid header `{header}`: {detail}")]
    InvalidHeader {
        header: &'static str,
        detail: String,
    },

    #[error("missing protocol field `{0}`")]
    MissingField(&'static str),

    #[error("remote protocol changed at `{context}`: {detail}")]
    ProtocolChanged {
        context: &'static str,
        detail: String,
    },

    #[error("remote api error {code}: {message}")]
    RemoteError { code: i64, message: String },

    #[error("timeout")]
    Timeout,

    #[error("unexpected status: {status}, body: {body}")]
    UnexpectedStatus { status: u16, body: String },
}

impl ApiError {
    pub fn invalid_header(header: &'static str, detail: impl std::fmt::Display) -> Self {
        Self::InvalidHeader {
            header,
            detail: detail.to_string(),
        }
    }

    pub fn protocol(context: &'static str, detail: impl std::fmt::Display) -> Self {
        Self::ProtocolChanged {
            context,
            detail: detail.to_string(),
        }
    }

    pub fn ws_request(detail: impl std::fmt::Display) -> Self {
        Self::WebSocketRequest(detail.to_string())
    }

    pub fn ws_send(detail: impl std::fmt::Display) -> Self {
        Self::WebSocketSend(detail.to_string())
    }

    pub fn ws_receive(detail: impl std::fmt::Display) -> Self {
        Self::WebSocketReceive(detail.to_string())
    }

    pub fn ws_connect(detail: impl std::fmt::Display) -> Self {
        Self::WebSocketConnect(detail.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_error_constructors() {
        let err = ApiError::invalid_header("Authorization", "missing");
        match err {
            ApiError::InvalidHeader { header, detail } => {
                assert_eq!(header, "Authorization");
                assert_eq!(detail, "missing");
            }
            _ => panic!("Expected ApiError::InvalidHeader"),
        }

        let err = ApiError::protocol("parsing", "unexpected char");
        match err {
            ApiError::ProtocolChanged { context, detail } => {
                assert_eq!(context, "parsing");
                assert_eq!(detail, "unexpected char");
            }
            _ => panic!("Expected ApiError::ProtocolChanged"),
        }

        let err = ApiError::ws_request("failed to send");
        match err {
            ApiError::WebSocketRequest(detail) => {
                assert_eq!(detail, "failed to send");
            }
            _ => panic!("Expected ApiError::WebSocketRequest"),
        }

        let err = ApiError::ws_send("closed");
        match err {
            ApiError::WebSocketSend(detail) => {
                assert_eq!(detail, "closed");
            }
            _ => panic!("Expected ApiError::WebSocketSend"),
        }

        let err = ApiError::ws_receive("corrupted");
        match err {
            ApiError::WebSocketReceive(detail) => {
                assert_eq!(detail, "corrupted");
            }
            _ => panic!("Expected ApiError::WebSocketReceive"),
        }

        let err = ApiError::ws_connect("timeout");
        match err {
            ApiError::WebSocketConnect(detail) => {
                assert_eq!(detail, "timeout");
            }
            _ => panic!("Expected ApiError::WebSocketConnect"),
        }
    }

    #[test]
    fn test_api_error_display() {
        let err = ApiError::Unauthorized;
        assert_eq!(err.to_string(), "unauthorized");

        let err = ApiError::RateLimited;
        assert_eq!(err.to_string(), "rate limited");

        let err = ApiError::Timeout;
        assert_eq!(err.to_string(), "timeout");

        let err = ApiError::MissingField("id");
        assert_eq!(err.to_string(), "missing protocol field `id`");
    }
}
