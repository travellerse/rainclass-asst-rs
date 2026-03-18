use reqwest::header::{COOKIE, HeaderMap, HeaderValue, USER_AGENT};

use rca_core::auth::AuthSession;

use crate::api::ApiError;

pub fn session_headers(
    host: &str,
    user_agent: &str,
    session: &AuthSession,
) -> Result<HeaderMap, ApiError> {
    let mut headers = HeaderMap::new();
    let cookie = format!("sessionid={}", session.access_token);
    tracing::debug!(
        token_len = session.access_token.len(),
        "attaching session cookie"
    );
    headers.insert(
        COOKIE,
        HeaderValue::from_str(&cookie).map_err(|err| ApiError::invalid_header("cookie", err))?,
    );
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(user_agent)
            .map_err(|err| ApiError::invalid_header("user-agent", err))?,
    );

    let origin = format!("https://{host}");
    headers.insert(
        reqwest::header::ORIGIN,
        HeaderValue::from_str(&origin).map_err(|err| ApiError::invalid_header("origin", err))?,
    );
    let referer = format!("{origin}/v2/web/index");
    headers.insert(
        reqwest::header::REFERER,
        HeaderValue::from_str(&referer).map_err(|err| ApiError::invalid_header("referer", err))?,
    );

    Ok(headers)
}
