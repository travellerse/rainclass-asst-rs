use std::path::{Path, PathBuf};

use bytes::Bytes;
use reqwest::header::{AUTHORIZATION, HeaderValue};
use serde_json::{Value, json};

use rca_core::app::ports::ApiPortError;
use rca_core::auth::AuthSession;

use super::YktApiPort;

fn sanitize_filename_component(input: &str) -> String {
    let replaced = input
        .replace(['\u{0000}', '/', '\\'], "_")
        .replace([':', '*', '?', '"', '<', '>', '|'], "_")
        .trim()
        .replace(' ', "_");

    let no_dot_segments = replaced
        .split('.')
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join(".");

    if no_dot_segments.is_empty() {
        "Presentation".to_string()
    } else {
        no_dot_segments
    }
}

fn build_safe_output_path(save_dir: &Path, file_name: &str) -> Result<PathBuf, std::io::Error> {
    let candidate = Path::new(file_name);
    if candidate.file_name().is_none() || candidate.components().count() != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "unsafe file name",
        ));
    }

    let base = save_dir.canonicalize()?;
    let out = save_dir.join(candidate);

    let out_parent = out
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent"))?
        .canonicalize()?;
    if !out_parent.starts_with(&base) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "path traversal detected",
        ));
    }

    Ok(out)
}

pub(super) async fn download_presentation(
    port: &YktApiPort,
    session: &AuthSession,
    presentation_id: u64,
    lesson_id: Option<u64>,
    save_dir: &Path,
) -> Result<PathBuf, ApiPortError> {
    let started_at = tokio::time::Instant::now();
    let span = tracing::debug_span!(
        target: "rca_infra.api",
        "download_presentation",
        host = %port.host,
        presentation_id = presentation_id,
        lesson_id = ?lesson_id
    );
    let _enter = span.enter();

    let mut headers = port
        .session_headers(session)
        .map_err(ApiPortError::protocol)?;

    if let Some(lid) = lesson_id {
        let checkin_url = format!("https://{}/api/v3/lesson/checkin", port.host);
        let response = port
            .client
            .post(&checkin_url)
            .headers(headers.clone())
            .json(&json!({
                "source": 5,
                "lessonId": lid.to_string(),
            }))
            .send()
            .await
            .map_err(|err| ApiPortError::request("lesson checkin", err))?;

        if let Some(auth_val) = response
            .headers()
            .get("set-auth")
            .or_else(|| response.headers().get("Set-Auth"))
            && let Ok(bearer) = auth_val.to_str()
        {
            let bearer = bearer.trim();
            if bearer.is_empty() || bearer.len() > 4096 {
                return Err(ApiPortError::protocol("invalid set-auth bearer token"));
            }
            if bearer.contains('\u{0000}') {
                return Err(ApiPortError::protocol("invalid set-auth bearer token"));
            }
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {bearer}"))
                    .map_err(|e| ApiPortError::protocol(format!("invalid bearer header: {e}")))?,
            );
        }
    }

    let fetch_url = format!(
        "https://{}/api/v3/lesson/presentation/fetch?presentation_id={}",
        port.host, presentation_id
    );
    let response = port
        .client
        .get(&fetch_url)
        .headers(headers)
        .send()
        .await
        .map_err(|err| ApiPortError::request("presentation fetch", err))?;

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(ApiPortError::protocol(
            "unauthorized: presentation fetch requires login",
        ));
    }

    let response = response
        .error_for_status()
        .map_err(|err| ApiPortError::request("presentation fetch HTTP error", err))?;
    let response_text = response
        .text()
        .await
        .map_err(|err| ApiPortError::request("presentation fetch text", err))?;

    let presentation_value: Value = serde_json::from_str(&response_text).map_err(|err| {
        tracing::error!(
            http_status = %status,
            response_len = response_text.len(),
            "presentation fetch decode failed"
        );
        ApiPortError::request("presentation fetch decode", err)
    })?;

    let presentation_data =
        YktApiPort::parse_api_ok(presentation_value).map_err(ApiPortError::protocol)?;
    let width = presentation_data
        .get("width")
        .and_then(Value::as_f64)
        .unwrap_or(1920.0);
    let height = presentation_data
        .get("height")
        .and_then(Value::as_f64)
        .unwrap_or(1080.0);
    let title = presentation_data
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Presentation");

    let Some(Value::Array(slides)) = presentation_data.get("slides") else {
        return Err(ApiPortError::protocol(
            "missing slides array in presentation data",
        ));
    };

    let mut slide_urls = Vec::new();
    for slide in slides {
        if let Some(cover_url) = slide.get("cover").and_then(Value::as_str)
            && !cover_url.is_empty()
        {
            slide_urls.push(cover_url.to_string());
        }
    }
    if slide_urls.is_empty() {
        return Err(ApiPortError::protocol(
            "no slide images found in presentation",
        ));
    }

    async fn download_slide_bytes(
        client: reqwest::Client,
        url: String,
    ) -> Result<Bytes, ApiPortError> {
        use tokio::time::{Duration, sleep, timeout};

        const PER_SLIDE_TIMEOUT_SECS: u64 = 30;
        const MAX_RETRIES: usize = 2;

        let mut attempt: usize = 0;
        loop {
            attempt += 1;
            let req_fut = async {
                client
                    .get(&url)
                    .send()
                    .await
                    .map_err(|e| ApiPortError::request("download slide image", e))?
                    .error_for_status()
                    .map_err(|e| ApiPortError::request("download slide image HTTP error", e))?
                    .bytes()
                    .await
                    .map_err(|e| ApiPortError::request("download slide image bytes", e))
            };

            match timeout(Duration::from_secs(PER_SLIDE_TIMEOUT_SECS), req_fut).await {
                Ok(Ok(bytes)) => return Ok(bytes),
                Ok(Err(err)) => {
                    if attempt > MAX_RETRIES {
                        tracing::warn!(attempt, url = %url, "slide download failed (no more retries)");
                        return Err(err);
                    }
                    let backoff = Duration::from_millis(200 * attempt as u64);
                    tracing::warn!(attempt, url = %url, backoff_ms = backoff.as_millis(), "slide download failed, retrying");
                    sleep(backoff).await;
                }
                Err(_) => {
                    if attempt > MAX_RETRIES {
                        return Err(ApiPortError::request(
                            "download slide image timeout",
                            format!("timeout after {PER_SLIDE_TIMEOUT_SECS}s"),
                        ));
                    }
                    let backoff = Duration::from_millis(200 * attempt as u64);
                    tracing::warn!(attempt, url = %url, backoff_ms = backoff.as_millis(), "slide download timed out, retrying");
                    sleep(backoff).await;
                }
            }
        }
    }

    let mut image_bytes_results: Vec<Bytes> = Vec::with_capacity(slide_urls.len());
    let chunk_size = 10;
    for chunk in slide_urls.chunks(chunk_size) {
        let mut tasks = Vec::new();
        for url in chunk {
            let client = port.client.clone();
            let url = url.clone();
            tasks.push(tokio::spawn(async move {
                download_slide_bytes(client, url).await
            }));
        }
        let chunk_results = futures_util::future::join_all(tasks).await;
        for res in chunk_results {
            let bytes = res.map_err(|_| ApiPortError::request("tokio join", "task panicked"))??;
            image_bytes_results.push(bytes);
        }
    }

    let safe_title = sanitize_filename_component(title);
    let file_name = format!(
        "{}_{}.pdf",
        safe_title,
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    );

    if !save_dir.exists() {
        tokio::fs::create_dir_all(save_dir)
            .await
            .map_err(|e| ApiPortError::request("create save dir", e))?;
    }
    let save_path = build_safe_output_path(save_dir, &file_name)
        .map_err(|e| ApiPortError::request("build save path", e))?;

    let pdf_bytes =
        YktApiPort::build_presentation_pdf_bytes(width as f32, height as f32, image_bytes_results)?;

    use std::fs::File;
    use std::io::Write;
    let tmp_path = save_path.with_extension("pdf.tmp");
    let write_result: Result<(), ApiPortError> = (|| {
        let mut file = std::io::BufWriter::new(
            File::create(&tmp_path).map_err(|e| ApiPortError::request("open pdf tmp file", e))?,
        );
        file.write_all(&pdf_bytes)
            .map_err(|e| ApiPortError::request("write pdf tmp", e))?;
        file.flush()
            .map_err(|e| ApiPortError::request("flush pdf tmp", e))?;
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(e);
    }

    tokio::fs::rename(&tmp_path, &save_path)
        .await
        .map_err(|e| ApiPortError::request("rename pdf tmp", e))?;

    tracing::debug!(
        bytes = pdf_bytes.len(),
        elapsed_ms = started_at.elapsed().as_millis(),
        "pdf saved"
    );

    Ok(save_path)
}
