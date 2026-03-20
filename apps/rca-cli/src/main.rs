use std::error::Error;

use clap::{Parser, Subcommand};
use image::DynamicImage;
use qrcode::QrCode;
use qrcode::render::unicode;
use rca_core::app::{
    AppCommand, AppConfigDto, AppEvent, AppQuery, AppQueryResult, AppService, TenantKind,
};
use rca_core::auth::AuthState;
use tokio::time::{Duration, Instant, sleep};
use tracing::info;

rust_i18n::i18n!("../../locales", fallback = "zh-CN");

#[derive(Debug, Parser)]
#[command(name = "rca-cli", about = "RainClassroom Assistant CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
}

#[derive(Debug, Subcommand)]
enum Command {
    Status,
    Login {
        #[arg(long, default_value_t = 20)]
        attempts: u32,
        #[arg(long, default_value_t = 1)]
        interval_secs: u64,
    },
    RefreshSession,
    CheckUpdate,
    Events {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    StartMonitor,
    StopMonitor,
    Monitor {
        #[arg(long)]
        duration_secs: Option<u64>,
        #[arg(long, short = 'd')]
        auto_download_ppt: bool,
    },
    Logout,
    /// View or update application configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Download a presentation as PDF
    DownloadPpt {
        #[arg(long)]
        presentation_id: u64,
        #[arg(long)]
        lesson_id: Option<u64>,
        #[arg(long, default_value = ".")]
        dir: String,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Get,
    SetTenant { tenant: String },
}

fn default_config() -> AppConfigDto {
    AppConfigDto::default()
}

fn auth_state_summary(state: &AuthState) -> String {
    match state {
        AuthState::LoggedOut => rust_i18n::t!("cli_auth_offline").to_string(),
        AuthState::WaitingQrScan { scene_id, .. } => {
            rust_i18n::t!("cli_auth_waiting_qr", scene_id = scene_id).to_string()
        }
        AuthState::WaitingConfirm { scene_id } => {
            rust_i18n::t!("cli_auth_waiting_confirm", scene_id = scene_id).to_string()
        }
        AuthState::LoggedIn { user_id } => {
            rust_i18n::t!("cli_auth_logged_in", user_id = user_id).to_string()
        }
        AuthState::Refreshing { user_id } => {
            rust_i18n::t!("cli_auth_refreshing", user_id = user_id).to_string()
        }
        AuthState::Failed { reason } => {
            rust_i18n::t!("cli_auth_failed", reason = reason).to_string()
        }
    }
}

fn format_state(result: AppQueryResult) -> Result<String, Box<dyn Error>> {
    let AppQueryResult::State(state) = result else {
        return Err("unexpected query result type for state".into());
    };

    let mut out = String::new();
    out.push_str(&format!(
        "auth_state      : {}\n",
        auth_state_summary(&state.auth_state)
    ));
    out.push_str(&format!("monitor_running : {}\n", state.monitor_running));
    out.push_str(&format!(
        "lessons         : {}\n",
        state.current_lessons.len()
    ));
    out.push_str(&format!(
        "recent_events   : {}\n",
        state.recent_events.len()
    ));
    if let Some(error) = state.last_error {
        out.push_str(&format!("last_error      : {error}\n"));
    }
    Ok(out)
}

fn print_state(result: AppQueryResult) -> Result<(), Box<dyn Error>> {
    print!("{}", format_state(result)?);
    Ok(())
}

fn format_login_qr_output(payload: &str) -> Vec<String> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return vec![rust_i18n::t!("cli_qr_empty").to_string()];
    }

    if trimmed.starts_with("<svg") {
        return vec![rust_i18n::t!("cli_qr_svg").to_string(), trimmed.to_string()];
    }

    match QrCode::new(trimmed.as_bytes()) {
        Ok(code) => {
            let rendered = code.render::<unicode::Dense1x2>().quiet_zone(true).build();
            vec![
                rust_i18n::t!("cli_qr_scan_prompt").to_string(),
                rendered,
                rust_i18n::t!("cli_qr_content", content = trimmed).to_string(),
            ]
        }
        Err(err) => vec![
            rust_i18n::t!("cli_qr_render_fail", err = err).to_string(),
            rust_i18n::t!("cli_qr_manual_prompt", content = trimmed).to_string(),
        ],
    }
}

fn decode_qr_from_image(image: DynamicImage) -> Option<String> {
    let gray = image.to_luma8();
    let mut prepared = rqrr::PreparedImage::prepare(gray);
    let grids = prepared.detect_grids();
    for grid in grids {
        if let Ok((_meta, content)) = grid.decode() {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

async fn resolve_terminal_qr_payload(payload: &str) -> String {
    let trimmed = payload.trim();
    if !trimmed.contains("showqrcode") {
        return trimmed.to_string();
    }

    let response = match reqwest::get(trimmed).await {
        Ok(resp) => resp,
        Err(_) => return trimmed.to_string(),
    };
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return trimmed.to_string(),
    };
    let image = match image::load_from_memory(&bytes) {
        Ok(img) => img,
        Err(_) => return trimmed.to_string(),
    };

    decode_qr_from_image(image).unwrap_or_else(|| trimmed.to_string())
}

fn print_login_qr(payload: &str) {
    for line in format_login_qr_output(payload) {
        println!("{line}");
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let default_level = match cli.verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };

    let _log_guards = rca_app::init_default_logger(default_level);
    let app = rca_app::bootstrap_core_app(rca_app::BootstrapOptions {
        default_config: default_config(),
        notifier_mode: rca_app::NotifierMode::Cli,
        startup: rca_app::StartupActions::cli_default(),
        storage_root: None,
    })
    .await?;

    match cli.command {
        Command::Status => {
            let state = app.handle_query(AppQuery::GetAppState).await?;
            print_state(state)?;
        }
        Command::Login {
            attempts,
            interval_secs,
        } => {
            app.handle_command(AppCommand::LoginByQr).await?;
            let state = app.handle_query(AppQuery::GetAppState).await?;
            let scene_id = match state {
                AppQueryResult::State(state) => match state.auth_state {
                    AuthState::WaitingQrScan { scene_id, token } => {
                        println!("scene_id        : {scene_id}");
                        println!("token           : {token}");
                        let terminal_payload = resolve_terminal_qr_payload(&token).await;
                        if terminal_payload != token {
                            println!("已自动解析为可直接扫码内容。\n");
                        }
                        print_login_qr(&terminal_payload);
                        println!("请在手机端确认登录。");
                        scene_id
                    }
                    AuthState::WaitingConfirm { scene_id } => scene_id,
                    other => {
                        return Err(
                            format!("unexpected auth state after login start: {other:?}").into(),
                        );
                    }
                },
                _ => return Err("unexpected query result type for state".into()),
            };
            let timeout_secs = (attempts as u64)
                .saturating_mul(interval_secs.max(1))
                .max(1);
            app.handle_command(AppCommand::AwaitLogin {
                scene_id,
                timeout_secs,
            })
            .await?;

            let state = app.handle_query(AppQuery::GetAppState).await?;
            let AppQueryResult::State(state) = state else {
                return Err("unexpected query result type for state".into());
            };

            match state.auth_state {
                AuthState::LoggedIn { user_id } => {
                    println!("登录成功: {user_id}");
                }
                AuthState::Failed { reason } => {
                    println!("登录失败: {reason}");
                }
                _ => {
                    println!("登录等待超时，请重试 login 命令。");
                }
            }
        }
        Command::RefreshSession => {
            app.handle_command(AppCommand::RefreshSession).await?;
            println!("会话刷新完成");
            let state = app.handle_query(AppQuery::GetAppState).await?;
            print_state(state)?;
        }
        Command::CheckUpdate => {
            let mut rx = app.subscribe_events();
            app.handle_command(AppCommand::CheckUpdate).await?;
            match tokio::time::timeout(Duration::from_millis(300), rx.recv()).await {
                Ok(Some(AppEvent::UpdateAvailable { version, url })) => {
                    println!("发现新版本: {version}");
                    println!("下载地址: {url}");
                }
                _ => println!("当前已是最新版本或未检测到更新事件"),
            }
        }
        Command::Events { limit } => {
            let result = app
                .handle_query(AppQuery::GetRecentEvents { limit })
                .await?;
            let AppQueryResult::Events(events) = result else {
                return Err("unexpected query result type for events".into());
            };
            if events.is_empty() {
                println!("暂无事件");
            } else {
                for event in events {
                    println!("{event:?}");
                }
            }
        }
        Command::StartMonitor => {
            app.handle_command(AppCommand::StartMonitor).await?;
            info!("监控已启动。");
            let state = app.handle_query(AppQuery::GetAppState).await?;
            print_state(state)?;
        }
        Command::StopMonitor => {
            app.handle_command(AppCommand::StopMonitor).await?;
            info!("监控已停止。");
        }
        Command::Monitor {
            duration_secs,
            auto_download_ppt,
        } => {
            let mut rx = app.subscribe_events();
            let mut downloaded_presentations = std::collections::HashSet::new();
            app.handle_command(AppCommand::StartMonitor).await?;
            if let Some(secs) = duration_secs {
                info!("监控已启动，将持续 {secs} 秒。按 Ctrl+C 可提前退出。");
            } else {
                info!("监控已启动（守护模式，将持续运行）。按 Ctrl+C 退出。");
            }

            let deadline = duration_secs.map(|secs| Instant::now() + Duration::from_secs(secs));
            loop {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        info!("收到 Ctrl+C，准备停止监控...");
                        break;
                    }
                    maybe_event = rx.recv() => {
                        if let Some(event) = maybe_event {
                            match &event {
                                rca_core::app::AppEvent::Notification(n) => {
                                    // INFO should be user-facing.
                                    tracing::info!("{}：{}", n.title, n.body);
                                }
                                rca_core::app::AppEvent::StateChanged(state) => {
                                    if let Some(err) = state.last_error.as_deref() {
                                        tracing::error!("{}", err);
                                    }
                                }
                                rca_core::app::AppEvent::PresentationDiscovered {
                                    lesson_id,
                                    presentation_id,
                                } => {
                                    if auto_download_ppt
                                        && !downloaded_presentations.contains(presentation_id)
                                    {
                                        downloaded_presentations.insert(*presentation_id);
                                        let pres_id = *presentation_id;
                                        let lid = lesson_id.0.get();
                                        let app_clone = app.clone();
                                        info!("自动下载新发现的 PPT: {}", pres_id);
                                        tokio::spawn(async move {
                                            let _ = app_clone
                                                .handle_command(AppCommand::DownloadPresentation {
                                                    presentation_id: pres_id,
                                                    lesson_id: Some(lid),
                                                    save_dir: std::path::PathBuf::from("./out"),
                                                })
                                                .await;
                                        });
                                    }
                                }
                                rca_core::app::AppEvent::UpdateAvailable { version, url } => {
                                    tracing::info!("发现新版本: {} ({})", version, url);
                                }
                            }
                        }
                    }
                    _ = sleep(Duration::from_millis(200)) => {
                        if let Some(d) = deadline
                            && Instant::now() >= d
                        {
                            break;
                        }
                    }
                }
            }

            app.handle_command(AppCommand::StopMonitor).await?;
            info!("监控已停止。");
        }
        Command::Logout => {
            app.handle_command(AppCommand::Logout).await?;
            info!("已登出并清除本地会话");
        }
        Command::Config { command } => match command {
            ConfigCommand::Get => {
                let state = app.handle_query(AppQuery::GetConfig).await?;
                if let AppQueryResult::Config(config) = state {
                    println!("当前配置: {config:#?}");
                }
            }
            ConfigCommand::SetTenant { tenant } => {
                let Some(kind) = TenantKind::parse_config_str(&tenant) else {
                    return Err(format!(
                        "不支持的服务器: {tenant}。支持的值: Rain/Hetang/Yangtze/YellowRiver"
                    )
                    .into());
                };
                let tenant = kind.as_config_string();

                let state = app.handle_query(AppQuery::GetConfig).await?;
                if let AppQueryResult::Config(mut config) = state {
                    config.tenant = kind;
                    app.handle_command(AppCommand::SaveConfig { config })
                        .await?;
                    println!("成功将服务器切换为 {}", tenant);
                    println!(
                        "请注意：您需要重启程序，且可能需要使用 `rca-cli logout` 重新登录才能完全生效。"
                    );
                }
            }
        },
        Command::DownloadPpt {
            presentation_id,
            lesson_id,
            dir,
        } => {
            let save_dir = std::path::PathBuf::from(dir);
            app.handle_command(AppCommand::DownloadPresentation {
                presentation_id,
                lesson_id,
                save_dir,
            })
            .await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Luma};
    use std::future::Future;

    #[test]
    fn auth_state_summary_covers_all_variants() {
        let s = auth_state_summary(&AuthState::LoggedOut);
        assert!(!s.trim().is_empty());

        let s = auth_state_summary(&AuthState::WaitingQrScan {
            scene_id: "1".to_string(),
            token: "t".to_string(),
        });
        assert!(s.contains('1'));

        let s = auth_state_summary(&AuthState::WaitingConfirm {
            scene_id: "2".to_string(),
        });
        assert!(s.contains('2'));

        let s = auth_state_summary(&AuthState::LoggedIn { user_id: 42 });
        assert!(s.contains("42"));

        let s = auth_state_summary(&AuthState::Refreshing { user_id: 7 });
        assert!(s.contains("7"));

        let s = auth_state_summary(&AuthState::Failed {
            reason: "oops".to_string(),
        });
        assert!(s.contains("oops"));
    }

    #[test]
    fn format_state_renders_expected_lines() {
        let dto = rca_core::app::AppState {
            auth_state: AuthState::LoggedOut,
            monitor_running: false,
            current_lessons: vec![],
            recent_events: vec![],
            last_error: Some("E".to_string()),
        };
        let s = format_state(AppQueryResult::State(dto)).expect("format_state ok");
        assert!(s.contains("auth_state"));
        assert!(s.contains("monitor_running"));
        assert!(s.contains("lessons"));
        assert!(s.contains("recent_events"));
        assert!(s.contains("last_error"));
        assert!(s.contains('E'));
        assert!(s.ends_with('\n'));
    }

    #[test]
    fn format_state_rejects_unexpected_query_result() {
        let err = format_state(AppQueryResult::Config(default_config())).unwrap_err();
        assert!(err.to_string().contains("unexpected"));
    }

    #[test]
    fn format_login_qr_output_empty_and_svg_are_handled() {
        let lines = format_login_qr_output("   ");
        assert!(!lines.is_empty());

        let lines = format_login_qr_output("<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>");
        assert!(lines.len() >= 2);
        assert!(lines[1].starts_with("<svg"));
    }

    #[test]
    fn format_login_qr_output_normal_payload_includes_content_line() {
        let payload = "hello";
        let lines = format_login_qr_output(payload);
        assert!(lines.iter().any(|l| l.contains(payload)));
    }

    fn run_async<F: Future>(f: F) -> F::Output {
        tokio::runtime::Runtime::new().expect("rt").block_on(f)
    }

    #[test]
    fn decode_qr_from_image_extracts_content() {
        let payload = "qr:test";
        let code = QrCode::new(payload.as_bytes()).expect("qrcode");

        // Create a small monochrome image: each module becomes a 3x3 block.
        let module_count = code.width();
        let scale: u32 = 3;
        let quiet: u32 = 4;
        let img_w = (module_count as u32 + quiet * 2) * scale;
        let img_h = img_w;

        let mut img: ImageBuffer<Luma<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(img_w, img_h, Luma([255]));
        for y in 0..module_count {
            for x in 0..module_count {
                if matches!(code[(x, y)], qrcode::types::Color::Dark) {
                    let px0 = (x as u32 + quiet) * scale;
                    let py0 = (y as u32 + quiet) * scale;
                    for dy in 0..scale {
                        for dx in 0..scale {
                            img.put_pixel(px0 + dx, py0 + dy, Luma([0]));
                        }
                    }
                }
            }
        }

        let dyn_img = DynamicImage::ImageLuma8(img);
        let decoded = decode_qr_from_image(dyn_img).expect("decoded");
        assert_eq!(decoded, payload);
    }

    #[test]
    fn resolve_terminal_qr_payload_passthrough_when_no_showqrcode() {
        let got = run_async(resolve_terminal_qr_payload("  hello  "));
        assert_eq!(got, "hello");
    }

    #[test]
    fn resolve_terminal_qr_payload_falls_back_on_request_error() {
        // This contains showqrcode so it will attempt a fetch, but the URL is invalid and should fail fast.
        let payload = "showqrcode://not-a-valid-url";
        let got = run_async(resolve_terminal_qr_payload(payload));
        assert_eq!(got, payload);
    }
}
