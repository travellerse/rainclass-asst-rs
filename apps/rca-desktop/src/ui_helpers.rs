use chrono::Local;
use rca_core::app::AppQueryResult;
use rca_core::auth::AuthState;
use rca_core::monitor::CoreEvent;

use crate::slint_generatedAppWindow::{EventRow as UiEventRow, LessonRow as UiLessonRow};

pub fn auth_status_text(state: &AuthState) -> String {
    match state {
        AuthState::LoggedOut => "未登录".to_string(),
        AuthState::WaitingQrScan { .. } => "等待扫码".to_string(),
        AuthState::WaitingConfirm { .. } => "等待确认".to_string(),
        AuthState::LoggedIn { user_id } => format!("已登录（{user_id}）"),
        AuthState::Refreshing { user_id } => format!("刷新中（{user_id}）"),
        AuthState::Failed { reason } => format!("失败：{reason}"),
    }
}

pub fn auth_status_kind(state: &AuthState) -> &'static str {
    match state {
        AuthState::LoggedOut => "offline",
        AuthState::WaitingQrScan { .. } | AuthState::WaitingConfirm { .. } => "waiting",
        AuthState::LoggedIn { .. } | AuthState::Refreshing { .. } => "online",
        AuthState::Failed { .. } => "error",
    }
}

pub fn lesson_status_text(status: &rca_core::domain::LessonStatus) -> &'static str {
    match status {
        rca_core::domain::LessonStatus::Scheduled => "未开始",
        rca_core::domain::LessonStatus::Running => "上课中",
        rca_core::domain::LessonStatus::Ended => "已结束",
    }
}

pub fn format_core_event(event: &CoreEvent) -> (&'static str, String) {
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

/// Update the Slint UI to reflect the current application state.
///
/// This function is synchronous since Slint callbacks execute on the
/// UI thread. It uses the controller's runtime to wait for the query result.
pub fn sync_ui_state(ui: &crate::AppWindow, controller: &crate::app_controller::AppController) {
    let result = controller.runtime.block_on(controller.get_state());
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

            let lesson_model: Vec<UiLessonRow> = state
                .current_lessons
                .iter()
                .map(|l| UiLessonRow {
                    course_name: l.course_name.clone().into(),
                    teacher_name: l.teacher_name.clone().into(),
                    status: lesson_status_text(&l.status).into(),
                })
                .collect();
            ui.set_lessons(slint::ModelRc::new(slint::VecModel::from(lesson_model)));

            let current_selected = ui.get_selected_lesson_index();
            let lesson_count = state.current_lessons.len() as i32;
            if lesson_count <= 0 {
                ui.set_selected_lesson_index(-1);
            } else if current_selected < 0 || current_selected >= lesson_count {
                ui.set_selected_lesson_index(0);
            }

            let event_model: Vec<UiEventRow> = state
                .recent_events
                .iter()
                .rev()
                .take(100)
                .map(|e| {
                    let (kind, message) = format_core_event(e);
                    UiEventRow {
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

/// Synchronize configuration values from the app to the UI form fields.
pub fn sync_config_to_ui(ui: &crate::AppWindow, controller: &crate::app_controller::AppController) {
    let result = controller.runtime.block_on(controller.get_config());
    if let Ok(AppQueryResult::Config(config)) = result {
        ui.set_setting_monitor_interval(config.monitor_interval_secs as i32);
        ui.set_setting_auto_checkin(config.auto_checkin_enabled);
        ui.set_setting_auto_answer(config.auto_answer_enabled);
        ui.set_setting_auto_answer_random_guess(config.auto_answer_random_guess);
        ui.set_setting_answer_delay(config.answer_delay_ms as i32);
        ui.set_setting_notify_enabled(config.notify_enabled);
        ui.set_setting_webhook_url(config.webhook_url.into());
        ui.set_setting_check_update_on_startup(config.check_update_on_startup);
        ui.set_setting_active_tenant(config.tenant.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rca_core::auth::AuthState;
    use rca_core::monitor::CoreEvent;

    // simple helpers tests
    #[test]
    fn auth_texts() {
        assert_eq!(auth_status_text(&AuthState::LoggedOut), "未登录");
        assert_eq!(auth_status_kind(&AuthState::LoggedOut), "offline");
    }

    #[test]
    fn format_event_warning() {
        let ev = CoreEvent::Warning {
            code: "X",
            message: "hi".into(),
        };
        let (kind, msg) = format_core_event(&ev);
        assert_eq!(kind, "warning");
        assert!(msg.contains("hi"));
    }
}
