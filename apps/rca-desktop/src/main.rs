#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::error::Error;
use std::sync::Arc;

use chrono::Local;
use rca_core::app::{
    AppCommand, AppConfigDto, AppQuery, AppQueryResult, AppService, CoreAppDeps, CoreAppService,
};
use rca_core::auth::AuthState;
use rca_core::monitor::CoreEvent;
use rca_infra::api::{TenantHost, YktApiPort, YktApiPortConfig};
use rca_infra::bridge::{
    CoreConfigStoreAdapter, CoreNotifierAdapter, CoreSessionStoreAdapter, CoreUpdateCheckerAdapter,
};
use rca_infra::notify::{LoggingNotifier, MultiNotifier, WebhookNotifier};

use rca_infra::storage::{
    AppPaths, ConfigRepository, JsonFileConfigRepository, JsonFileSessionRepository,
    KeyringCredentialStore,
};
use rca_infra::update::GithubReleaseChecker;
use tracing_subscriber::EnvFilter;

slint::include_modules!();

fn default_config() -> AppConfigDto {
    AppConfigDto {
        monitor_interval_secs: 5,
        auto_checkin_enabled: true,
        auto_answer_enabled: true,
        answer_delay_ms: 500,
        notify_enabled: true,
        webhook_url: String::new(),
        check_update_on_startup: true,
        tenant: "Hetang".to_string(),
        auth_state_hint: None,
    }
}

fn auth_status_text(state: &AuthState) -> String {
    match state {
        AuthState::LoggedOut => "未登录".to_string(),
        AuthState::WaitingQrScan { .. } => "等待扫码".to_string(),
        AuthState::WaitingConfirm { .. } => "等待确认".to_string(),
        AuthState::LoggedIn { user_id } => format!("已登录（{user_id}）"),
        AuthState::Refreshing { user_id } => format!("刷新中（{user_id}）"),
        AuthState::Failed { reason } => format!("失败：{reason}"),
    }
}

fn auth_status_kind(state: &AuthState) -> &'static str {
    match state {
        AuthState::LoggedOut => "offline",
        AuthState::WaitingQrScan { .. } | AuthState::WaitingConfirm { .. } => "waiting",
        AuthState::LoggedIn { .. } | AuthState::Refreshing { .. } => "online",
        AuthState::Failed { .. } => "error",
    }
}

fn format_core_event(event: &CoreEvent) -> (&'static str, String) {
    match event {
        CoreEvent::MonitorStarted { at } => {
            let local = at.with_timezone(&Local);
            (
                "success",
                format!("监控已启动 ({})", local.format("%H:%M:%S")),
            )
        }
        CoreEvent::MonitorStopped { at } => {
            let local = at.with_timezone(&Local);
            ("info", format!("监控已停止 ({})", local.format("%H:%M:%S")))
        }
        CoreEvent::LessonDiscovered { lesson } => (
            "info",
            format!("发现课程：{} ({})", lesson.course_name, lesson.teacher_name),
        ),
        CoreEvent::ProblemDiscovered { problem } => {
            ("warning", format!("收到题目：{}", problem.title))
        }
        CoreEvent::CheckinDiscovered {
            lesson_id,
            checkin_id,
        } => (
            "warning",
            format!("签到已开启 (课程 {:?}, 签到 {:?})", lesson_id, checkin_id),
        ),
        CoreEvent::AutoAnswerSubmitted {
            lesson_id,
            problem_id,
        } => (
            "success",
            format!("自动答题完成 (课程 {:?}, 题目 {:?})", lesson_id, problem_id),
        ),
        CoreEvent::AutoCheckinSubmitted {
            lesson_id,
            checkin_id,
        } => (
            "success",
            format!("自动签到完成 (课程 {:?}, 签到 {:?})", lesson_id, checkin_id),
        ),
        CoreEvent::DanmuPublished {
            lesson_id,
            user_name,
            content,
        } => (
            "info",
            format!(
                "实时弹幕 (课程 {:?}) {}: {}",
                lesson_id,
                user_name.as_deref().unwrap_or("未知"),
                content
            ),
        ),
        CoreEvent::CallPaused {
            lesson_id,
            target_name,
        } => (
            "warning",
            format!(
                "老师发起了点名！点名目标：{} (课程 {:?})",
                target_name, lesson_id
            ),
        ),
        CoreEvent::PresentationUpdated {
            lesson_id,
            presentation_id,
        } => (
            "info",
            format!(
                "收到新的 PPT 页面: {} (课程 {:?})",
                presentation_id, lesson_id
            ),
        ),
        CoreEvent::Warning { code, message } => ("warning", format!("[{code}] {message}")),
        CoreEvent::Error { code, message } => ("error", format!("[{code}] {message}")),
    }
}

fn lesson_status_text(status: &rca_core::domain::LessonStatus) -> &'static str {
    match status {
        rca_core::domain::LessonStatus::Scheduled => "未开始",
        rca_core::domain::LessonStatus::Running => "上课中",
        rca_core::domain::LessonStatus::Ended => "已结束",
    }
}

/// Push the full app state to all UI properties.
fn sync_ui_state(ui: &AppWindow, app: &CoreAppService, runtime: &tokio::runtime::Runtime) {
    let result = runtime.block_on(app.handle_query(AppQuery::GetAppState));
    match result {
        Ok(AppQueryResult::State(state)) => {
            ui.set_auth_status_text(auth_status_text(&state.auth_state).into());
            ui.set_auth_status_kind(auth_status_kind(&state.auth_state).into());

            ui.set_monitor_running(state.monitor_running);
            ui.set_monitor_status_text(
                if state.monitor_running {
                    "运行中"
                } else {
                    "未启动"
                }
                .into(),
            );
            ui.set_last_error_text(state.last_error.unwrap_or_default().into());

            // Sync lessons
            let lesson_model: Vec<LessonRow> = state
                .current_lessons
                .iter()
                .map(|l| LessonRow {
                    course_name: l.course_name.clone().into(),
                    teacher_name: l.teacher_name.clone().into(),
                    status: lesson_status_text(&l.status).into(),
                })
                .collect();
            ui.set_lessons(slint::ModelRc::new(slint::VecModel::from(lesson_model)));

            // Sync events
            let event_model: Vec<EventRow> = state
                .recent_events
                .iter()
                .rev()
                .take(100)
                .map(|e| {
                    let (kind, message) = format_core_event(e);
                    EventRow {
                        timestamp: Local::now().format("%H:%M:%S").to_string().into(),
                        kind: kind.into(),
                        message: message.into(),
                    }
                })
                .collect();
            ui.set_events(slint::ModelRc::new(slint::VecModel::from(event_model)));
        }
        Ok(_) => {
            ui.set_last_error_text("状态查询返回类型异常".into());
        }
        Err(error) => {
            ui.set_last_error_text(format!("状态查询失败: {error}").into());
        }
    }
}

fn sync_config_to_ui(ui: &AppWindow, app: &CoreAppService, runtime: &tokio::runtime::Runtime) {
    let result = runtime.block_on(app.handle_query(AppQuery::GetConfig));
    if let Ok(AppQueryResult::Config(config)) = result {
        ui.set_setting_monitor_interval(config.monitor_interval_secs as i32);
        ui.set_setting_auto_checkin(config.auto_checkin_enabled);
        ui.set_setting_auto_answer(config.auto_answer_enabled);
        ui.set_setting_answer_delay(config.answer_delay_ms as i32);
        ui.set_setting_notify_enabled(config.notify_enabled);
        ui.set_setting_webhook_url(config.webhook_url.into());
        ui.set_setting_check_update_on_startup(config.check_update_on_startup);
        ui.set_setting_active_tenant(config.tenant.into());
    }
}

/// Run an async AppCommand in the background, then refresh UI.
fn spawn_command(
    app: Arc<CoreAppService>,
    runtime: Arc<tokio::runtime::Runtime>,
    ui_handle: slint::Weak<AppWindow>,
    cmd: AppCommand,
) {
    let _ = std::thread::spawn(move || {
        if let Err(e) = runtime.block_on(app.handle_command(cmd)) {
            let app2 = app.clone();
            let runtime2 = runtime.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_handle.upgrade() {
                    ui.set_last_error_text(format!("{e}").into());
                    sync_ui_state(&ui, &app2, &runtime2);
                }
            });
            return;
        }
        let app2 = app.clone();
        let runtime2 = runtime.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                sync_ui_state(&ui, &app2, &runtime2);
            }
        });
    });
}

/// Run the login flow in a background thread (start QR + wait).
fn spawn_login(
    app: Arc<CoreAppService>,
    runtime: Arc<tokio::runtime::Runtime>,
    ui_handle: slint::Weak<AppWindow>,
) {
    let _ = std::thread::spawn(move || {
        // Step 1: Initiate QR login
        if let Err(e) = runtime.block_on(app.handle_command(AppCommand::LoginByQr)) {
            let _ = slint::invoke_from_event_loop({
                let ui_handle = ui_handle.clone();
                move || {
                    if let Some(ui) = ui_handle.upgrade() {
                        ui.set_last_error_text(format!("发起登录失败: {e}").into());
                    }
                }
            });
            return;
        }

        // Step 2: Get scene_id
        let scene_id = match runtime.block_on(app.handle_query(AppQuery::GetAppState)) {
            Ok(AppQueryResult::State(state)) => match state.auth_state {
                AuthState::WaitingQrScan { scene_id, .. } => scene_id,
                AuthState::WaitingConfirm { scene_id } => scene_id,
                _ => {
                    let _ = slint::invoke_from_event_loop({
                        let ui_handle = ui_handle.clone();
                        move || {
                            if let Some(ui) = ui_handle.upgrade() {
                                ui.set_last_error_text("发起登录后未获取到 scene_id".into());
                            }
                        }
                    });
                    return;
                }
            },
            _ => return,
        };

        // Refresh UI to show "waiting" state
        {
            let app = app.clone();
            let runtime = runtime.clone();
            let _ = slint::invoke_from_event_loop({
                let ui_handle = ui_handle.clone();
                move || {
                    if let Some(ui) = ui_handle.upgrade() {
                        sync_ui_state(&ui, &app, &runtime);
                    }
                }
            });
        }

        // Step 3: Wait for login confirmation
        let _ = runtime.block_on(app.handle_command(AppCommand::WaitLogin {
            scene_id,
            timeout_secs: 20,
        }));

        // Final refresh
        let app2 = app.clone();
        let runtime2 = runtime.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                sync_ui_state(&ui, &app2, &runtime2);
            }
        });
    });
}

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let runtime = Arc::new(tokio::runtime::Runtime::new()?);

    let paths = AppPaths::detect()?;
    let config_repo = Arc::new(JsonFileConfigRepository::new(paths.config_file));
    let session_repo = Arc::new(JsonFileSessionRepository::new(paths.session_file));
    let credential_store = Arc::new(KeyringCredentialStore);

    // Read config manually or default to Hetang for ApiPort and Webhook initialization
    let initial_config = runtime.block_on(config_repo.load()).ok();

    let initial_tenant = initial_config
        .as_ref()
        .map(|cfg| match cfg.active_tenant {
            rca_infra::storage::TenantKind::Rain => TenantHost::Rain,
            rca_infra::storage::TenantKind::Hetang => TenantHost::Hetang,
            rca_infra::storage::TenantKind::Yangtze => TenantHost::Yangtze,
            rca_infra::storage::TenantKind::YellowRiver => TenantHost::YellowRiver,
        })
        .unwrap_or(TenantHost::Hetang);

    let mut notifiers: Vec<Box<dyn rca_infra::notify::Notifier>> = vec![Box::new(LoggingNotifier)];

    if let Some(cfg) = initial_config.as_ref()
        && !cfg.webhook_url.is_empty()
    {
        notifiers.push(Box::new(WebhookNotifier::new(&cfg.webhook_url)));
    }

    let notifier = Arc::new(MultiNotifier::new(notifiers));

    let update_checker = Arc::new(GithubReleaseChecker::new(
        "travellerse",
        "RainClassroomAssistant",
    )?);

    // ── Create UI ──
    let ui = AppWindow::new()?;

    let api_port: Arc<dyn rca_core::app::ports::ApiPort> =
        Arc::new(YktApiPort::new(YktApiPortConfig {
            tenant: initial_tenant,
            timeout_secs: 15,
        })?);

    let config_port = Arc::new(CoreConfigStoreAdapter::new(config_repo));
    let session_port = Arc::new(CoreSessionStoreAdapter::new(session_repo, credential_store));
    let notify_port = Arc::new(CoreNotifierAdapter::new(notifier));
    let update_port = Arc::new(CoreUpdateCheckerAdapter::new(update_checker));

    let app = Arc::new(CoreAppService::new(
        CoreAppDeps {
            api: api_port,
            config_store: config_port,
            session_store: session_port,
            notifier: notify_port,
            update_checker: update_port,
        },
        default_config(),
    ));

    // ── Bootstrap: load config & restore session ──
    let _ = runtime.block_on(app.handle_command(AppCommand::LoadConfig));
    let _ = runtime.block_on(app.handle_command(AppCommand::RestoreSession));
    let _ = runtime.block_on(app.handle_command(AppCommand::RefreshSession));

    // Check update on startup if configured
    let startup_should_check_update = runtime
        .block_on(app.handle_query(AppQuery::GetConfig))
        .ok()
        .and_then(|result| match result {
            AppQueryResult::Config(config) => Some(config.check_update_on_startup),
            _ => None,
        })
        .unwrap_or(false);
    if startup_should_check_update {
        let _ = runtime.block_on(app.handle_command(AppCommand::CheckUpdate));
    }

    // ── Create UI Done above ──

    // Initial sync
    sync_ui_state(&ui, &app, &runtime);
    sync_config_to_ui(&ui, &app, &runtime);

    // ── Bind callbacks ──

    // Login
    ui.on_login_clicked({
        let app = app.clone();
        let runtime = runtime.clone();
        let ui_handle = ui.as_weak();
        move || {
            spawn_login(app.clone(), runtime.clone(), ui_handle.clone());
        }
    });

    // Logout
    ui.on_logout_clicked({
        let app = app.clone();
        let runtime = runtime.clone();
        let ui_handle = ui.as_weak();
        move || {
            spawn_command(
                app.clone(),
                runtime.clone(),
                ui_handle.clone(),
                AppCommand::Logout,
            );
        }
    });

    // Start monitor
    ui.on_start_monitor_clicked({
        let app = app.clone();
        let runtime = runtime.clone();
        let ui_handle = ui.as_weak();
        move || {
            spawn_command(
                app.clone(),
                runtime.clone(),
                ui_handle.clone(),
                AppCommand::StartMonitor,
            );
        }
    });

    // Stop monitor
    ui.on_stop_monitor_clicked({
        let app = app.clone();
        let runtime = runtime.clone();
        let ui_handle = ui.as_weak();
        move || {
            spawn_command(
                app.clone(),
                runtime.clone(),
                ui_handle.clone(),
                AppCommand::StopMonitor,
            );
        }
    });

    // Check update
    ui.on_check_update_clicked({
        let app = app.clone();
        let runtime = runtime.clone();
        let ui_handle = ui.as_weak();
        move || {
            spawn_command(
                app.clone(),
                runtime.clone(),
                ui_handle.clone(),
                AppCommand::CheckUpdate,
            );
        }
    });

    // Save config
    ui.on_save_config_clicked({
        let app = app.clone();
        let runtime = runtime.clone();
        let ui_handle = ui.as_weak();
        move || {
            let ui = match ui_handle.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            let config = AppConfigDto {
                monitor_interval_secs: ui.get_setting_monitor_interval().max(1) as u64,
                auto_checkin_enabled: ui.get_setting_auto_checkin(),
                auto_answer_enabled: ui.get_setting_auto_answer(),
                answer_delay_ms: ui.get_setting_answer_delay().max(0) as u64,
                notify_enabled: ui.get_setting_notify_enabled(),
                webhook_url: ui.get_setting_webhook_url().to_string(),
                check_update_on_startup: ui.get_setting_check_update_on_startup(),
                tenant: ui.get_setting_active_tenant().to_string(),
                auth_state_hint: None,
            };
            spawn_command(
                app.clone(),
                runtime.clone(),
                ui_handle.clone(),
                AppCommand::SaveConfig { config },
            );
        }
    });

    // ── Start background event subscription ──
    {
        let mut rx = app.subscribe_events();
        let app = app.clone();
        let runtime_ref = runtime.clone();
        let ui_handle = ui.as_weak();

        std::thread::spawn(move || {
            while let Some(_event) = runtime_ref.block_on(rx.recv()) {
                let app = app.clone();
                let runtime = runtime_ref.clone();
                let ui_handle = ui_handle.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_handle.upgrade() {
                        sync_ui_state(&ui, &app, &runtime);
                    }
                });
            }
        });
    }

    ui.run()?;
    Ok(())
}
