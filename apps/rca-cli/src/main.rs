use std::error::Error;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use image::DynamicImage;
use qrcode::QrCode;
use qrcode::render::unicode;
use rca_core::app::{
    AppCommand, AppConfigDto, AppEvent, AppQuery, AppQueryResult, AppService, CoreAppDeps,
    CoreAppService,
};
use rca_core::auth::AuthState;
use rca_infra::api::{YktApiPort, YktApiPortConfig};
use rca_infra::bridge::{
    CoreConfigStoreAdapter, CoreNotifierAdapter, CoreSessionStoreAdapter, CoreUpdateCheckerAdapter,
};
use rca_infra::notify::LoggingNotifier;

use rca_infra::storage::{
    AppPaths, ConfigRepository, JsonFileConfigRepository, JsonFileSessionRepository,
    KeyringCredentialStore,
};
use rca_infra::update::GithubReleaseChecker;
use tokio::time::{Duration, Instant, sleep};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "rca-cli", about = "RainClassroom Assistant CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(short, long, global = true)]
    verbose: bool,
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
    },
    Logout,
    /// View or update application configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
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
        AuthState::LoggedOut => "未登录".to_string(),
        AuthState::WaitingQrScan { scene_id, .. } => format!("等待扫码 (scene_id={scene_id})"),
        AuthState::WaitingConfirm { scene_id } => format!("等待确认 (scene_id={scene_id})"),
        AuthState::LoggedIn { user_id } => format!("已登录 ({user_id})"),
        AuthState::Refreshing { user_id } => format!("刷新会话中 ({user_id})"),
        AuthState::Failed { reason } => format!("失败: {reason}"),
    }
}

fn print_state(result: AppQueryResult) -> Result<(), Box<dyn Error>> {
    let AppQueryResult::State(state) = result else {
        return Err("unexpected query result type for state".into());
    };

    println!(
        "auth_state      : {}",
        auth_state_summary(&state.auth_state)
    );
    println!("monitor_running : {}", state.monitor_running);
    println!("lessons         : {}", state.current_lessons.len());
    println!("recent_events   : {}", state.recent_events.len());
    if let Some(error) = state.last_error {
        println!("last_error      : {error}");
    }
    Ok(())
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
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        println!("二维码内容为空，无法渲染。");
        return;
    }

    if trimmed.starts_with("<svg") {
        println!("收到 SVG 内容，终端二维码渲染跳过；可复制以下内容到浏览器查看：");
        println!("{trimmed}");
        return;
    }

    match QrCode::new(trimmed.as_bytes()) {
        Ok(code) => {
            let rendered = code.render::<unicode::Dense1x2>().quiet_zone(true).build();
            println!("请使用微信扫码（终端二维码）：\n");
            println!("{rendered}");
            println!("扫码内容: {trimmed}");
        }
        Err(err) => {
            println!("二维码渲染失败: {err}");
            println!("请手动打开/复制扫码内容: {trimmed}");
        }
    }
}

async fn bootstrap_app() -> Result<Arc<CoreAppService>, Box<dyn Error>> {
    let paths = AppPaths::detect()?;
    let config_repo = Arc::new(JsonFileConfigRepository::new(paths.config_file));
    let session_repo = Arc::new(JsonFileSessionRepository::new(paths.session_file));
    let credential_store = Arc::new(KeyringCredentialStore);
    let notifier = Arc::new(LoggingNotifier);
    let update_checker = Arc::new(GithubReleaseChecker::new(
        "travellerse",
        "RainClassroomAssistant",
    )?);

    let initial_config = config_repo.load().await.ok();
    let initial_tenant = initial_config
        .as_ref()
        .map(|cfg| match cfg.active_tenant {
            rca_infra::storage::TenantKind::Rain => rca_infra::api::TenantHost::Rain,
            rca_infra::storage::TenantKind::Hetang => rca_infra::api::TenantHost::Hetang,
            rca_infra::storage::TenantKind::Yangtze => rca_infra::api::TenantHost::Yangtze,
            rca_infra::storage::TenantKind::YellowRiver => rca_infra::api::TenantHost::YellowRiver,
        })
        .unwrap_or(rca_infra::api::TenantHost::Hetang);

    let api_port: Arc<dyn rca_core::app::ports::ApiPort> =
        Arc::new(YktApiPort::new(YktApiPortConfig {
            tenant: initial_tenant,
            timeout_secs: 15,
        })?);

    let monitor = Arc::new(rca_core::monitor::CoreMonitorEngine::new(api_port.clone()));

    let app = Arc::new(CoreAppService::new(
        CoreAppDeps {
            api: api_port,
            config_store: Arc::new(CoreConfigStoreAdapter::new(config_repo)),
            session_store: Arc::new(CoreSessionStoreAdapter::new(session_repo, credential_store)),
            notifier: Arc::new(CoreNotifierAdapter::new(notifier)),
            update_checker: Arc::new(CoreUpdateCheckerAdapter::new(update_checker)),
            monitor_engine: monitor,
        },
        default_config(),
    ));

    Ok(app)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let default_level = if cli.verbose { "debug" } else { "info" };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level)),
        )
        .init();

    let app = bootstrap_app().await?;

    app.handle_command(AppCommand::LoadConfig).await?;
    app.handle_command(AppCommand::RestoreSession).await?;

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
            app.handle_command(AppCommand::WaitLogin {
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
            info!("监控已启动");
            let state = app.handle_query(AppQuery::GetAppState).await?;
            print_state(state)?;
        }
        Command::StopMonitor => {
            app.handle_command(AppCommand::StopMonitor).await?;
            info!("监控已停止");
        }
        Command::Monitor { duration_secs } => {
            let mut rx = app.subscribe_events();
            app.handle_command(AppCommand::StartMonitor).await?;
            if let Some(secs) = duration_secs {
                info!("监控已启动，持续 {secs} 秒。按 Ctrl+C 可提前退出。");
            } else {
                info!("监控已启动 (守护模式，无限期运行)。按 Ctrl+C 退出。");
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
                                rca_core::app::AppEvent::StateChanged(state) => {
                                    tracing::debug!("App state synchronized: {:?}", state);
                                    if let Some(err) = state.last_error.as_deref() {
                                        tracing::error!("Core Engine Error: {}", err);
                                    }
                                }
                                rca_core::app::AppEvent::Notification(n) => {
                                    tracing::info!("System Notification - {}: {}", n.title, n.body);
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
            info!("监控已停止");
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
                let valid_tenants = [
                    "rain",
                    "hetang",
                    "yangtze",
                    "yellow-river",
                    "Rain",
                    "Hetang",
                    "Yangtze",
                    "YellowRiver",
                ];
                if !valid_tenants.contains(&tenant.as_str()) {
                    return Err(
                        format!("不支持的服务器: {tenant}。支持的值: {valid_tenants:?}").into(),
                    );
                }

                // Title case the tenant for uniform config behavior across UI/CLI.
                let tenant = match tenant.to_lowercase().as_str() {
                    "rain" => "雨课堂".to_string(),
                    "hetang" => "Hetang".to_string(),
                    "yangtze" => "Yangtze".to_string(),
                    "yellow-river" => "YellowRiver".to_string(),
                    _ => unreachable!(),
                };

                let state = app.handle_query(AppQuery::GetConfig).await?;
                if let AppQueryResult::Config(mut config) = state {
                    config.tenant = tenant.clone();
                    app.handle_command(AppCommand::SaveConfig { config })
                        .await?;
                    println!("成功将服务器切换为 {}", tenant);
                    println!(
                        "请注意：您需要重启程序，且可能需要使用 `rca-cli logout` 重新登录才能完全生效。"
                    );
                }
            }
        },
    }

    Ok(())
}
