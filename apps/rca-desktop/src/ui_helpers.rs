use chrono::Local;
use rca_core::app::AppQueryResult;
use rca_core::app::AppState;
use rca_core::auth::AuthState;
use rca_core::monitor::CoreEvent;
use slint::ComponentHandle;

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
        CoreEvent::SlideNavigated {
            lesson_id,
            presentation_id: _,
            slide_id,
            slide_index,
        } => (
            "info",
            format!(
                "切换幻灯片: 页面 {} (id: {}) (课程 {:?})",
                slide_index, slide_id, lesson_id
            ),
        ),
        CoreEvent::ProblemUnlocked {
            lesson_id,
            problem_id,
        } => (
            "warning",
            format!("题目已解锁: {} (课程 {:?})", problem_id.0.get(), lesson_id),
        ),
        CoreEvent::Warning { code, message } => ("warning", format!("[{code}] {message}")),
        CoreEvent::Error { code, message } => ("error", format!("[{code}] {message}")),
    }
}

pub fn apply_state_to_ui(ui: &crate::AppWindow, state: &AppState) {
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
    ui.set_last_error_text(state.last_error.clone().unwrap_or_default().into());

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

/// Update the Slint UI to reflect the current application state.
///
/// This function is synchronous since Slint callbacks execute on the
/// UI thread. It uses the controller's runtime to wait for the query result.
pub fn sync_ui_state(ui: &crate::AppWindow, controller: &crate::app_controller::AppController) {
    let ui_handle = ui.as_weak();
    let controller = controller.clone();
    controller.clone().spawn_task(async move {
        let state = match controller.get_state().await {
            Ok(AppQueryResult::State(state)) => Ok(state),
            Ok(_) => Err("状态查询返回类型异常".to_string()),
            Err(e) => Err(format!("状态查询失败: {e}")),
        };
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                match state {
                    Ok(state) => apply_state_to_ui(&ui, &state),
                    Err(err) => ui.set_last_error_text(err.into()),
                }
            }
        });
    });
}

/// Synchronize configuration values from the app to the UI form fields.
pub fn sync_config_to_ui(ui: &crate::AppWindow, controller: &crate::app_controller::AppController) {
    let ui_handle = ui.as_weak();
    let controller = controller.clone();
    controller.clone().spawn_task(async move {
        let config = match controller.get_config().await {
            Ok(AppQueryResult::Config(config)) => Ok(config),
            Ok(_) => Err("配置查询返回类型异常".to_string()),
            Err(e) => Err(format!("配置查询失败: {e}")),
        };
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                match config {
                    Ok(config) => {
                        ui.set_setting_monitor_interval(config.monitor_interval_secs as i32);
                        ui.set_setting_auto_checkin(config.auto_checkin_enabled);
                        ui.set_setting_auto_answer(config.auto_answer_enabled);
                        ui.set_setting_auto_answer_random_guess(config.auto_answer_random_guess);
                        ui.set_setting_answer_delay(config.answer_delay_ms as i32);
                        ui.set_setting_notify_enabled(config.notify_enabled);
                        ui.set_setting_webhook_url(config.webhook_url.into());
                        ui.set_setting_check_update_on_startup(config.check_update_on_startup);
                        ui.set_setting_active_tenant(config.tenant.as_config_string().into());
                    }
                    Err(err) => ui.set_last_error_text(err.into()),
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rca_core::auth::AuthState;
    use rca_core::domain::LessonStatus;
    use rca_core::domain::{
        BlankAnswer, CourseId, Lesson, LessonId, Problem, ProblemId, ProblemOption, ProblemType,
    };
    use rca_core::monitor::CoreEvent;
    use std::num::NonZeroU64;

    // simple helpers tests
    #[test]
    fn auth_texts() {
        assert_eq!(auth_status_text(&AuthState::LoggedOut), "未登录");
        assert_eq!(auth_status_kind(&AuthState::LoggedOut), "offline");
    }

    #[test]
    fn auth_kinds_cover_all_states() {
        assert_eq!(auth_status_kind(&AuthState::LoggedOut), "offline");

        assert_eq!(
            auth_status_kind(&AuthState::WaitingQrScan {
                scene_id: "1".to_string(),
                token: "t".to_string(),
            }),
            "waiting"
        );
        assert_eq!(
            auth_status_kind(&AuthState::WaitingConfirm {
                scene_id: "2".to_string(),
            }),
            "waiting"
        );

        assert_eq!(
            auth_status_kind(&AuthState::LoggedIn { user_id: 42 }),
            "online"
        );
        assert_eq!(
            auth_status_kind(&AuthState::Refreshing { user_id: 42 }),
            "online"
        );

        assert_eq!(
            auth_status_kind(&AuthState::Failed {
                reason: "x".to_string()
            }),
            "error"
        );
    }

    #[test]
    fn lesson_status_texts_cover_all_statuses() {
        assert_eq!(lesson_status_text(&LessonStatus::Scheduled), "未开始");
        assert_eq!(lesson_status_text(&LessonStatus::Running), "上课中");
        assert_eq!(lesson_status_text(&LessonStatus::Ended), "已结束");
    }

    #[test]
    fn format_event_error_kind_and_message() {
        let ev = CoreEvent::Error {
            code: "E",
            message: "boom".into(),
        };
        let (kind, msg) = format_core_event(&ev);
        assert_eq!(kind, "error");
        assert!(msg.contains("boom"));
        assert!(msg.contains("E"));
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

    fn nz(v: u64) -> NonZeroU64 {
        NonZeroU64::new(v).expect("non-zero")
    }

    fn sample_lesson() -> Lesson {
        Lesson {
            lesson_id: LessonId(nz(1)),
            course_id: CourseId(nz(2)),
            course_name: "Rust 101".to_string(),
            teacher_name: "Alice".to_string(),
            started_at: None,
            ended_at: None,
            status: LessonStatus::Running,
        }
    }

    fn sample_problem() -> Problem {
        Problem {
            lesson_id: LessonId(nz(1)),
            problem_id: ProblemId(nz(3)),
            problem_type: ProblemType::SingleChoice,
            title: "选择题".to_string(),
            options: vec![ProblemOption {
                option_id: "A".to_string(),
                text: "答案A".to_string(),
            }],
            correct_answers: vec!["A".to_string()],
            blanks: vec![BlankAnswer {
                accepted_values: vec!["foo".to_string()],
            }],
            limit_secs: Some(30),
            published_at: chrono::Utc::now(),
            deadline_at: None,
        }
    }

    #[test]
    fn format_event_monitor_started_and_stopped() {
        let ev1 = CoreEvent::MonitorStarted {
            at: chrono::Utc::now(),
        };
        let (kind1, msg1) = format_core_event(&ev1);
        assert_eq!(kind1, "success");
        assert!(msg1.contains("监控已启动"));

        let ev2 = CoreEvent::MonitorStopped {
            at: chrono::Utc::now(),
        };
        let (kind2, msg2) = format_core_event(&ev2);
        assert_eq!(kind2, "info");
        assert!(msg2.contains("监控已停止"));
    }

    #[test]
    fn format_event_lesson_and_problem_discovered() {
        let lesson = sample_lesson();
        let (kind1, msg1) = format_core_event(&CoreEvent::LessonDiscovered { lesson });
        assert_eq!(kind1, "info");
        assert!(msg1.contains("发现课程"));
        assert!(msg1.contains("Rust 101"));
        assert!(msg1.contains("Alice"));

        let problem = sample_problem();
        let (kind2, msg2) = format_core_event(&CoreEvent::ProblemDiscovered { problem });
        assert_eq!(kind2, "warning");
        assert!(msg2.contains("收到题目"));
        assert!(msg2.contains("选择题"));
    }

    #[test]
    fn format_event_checkin_and_auto_actions() {
        let lesson_id = LessonId(nz(10));
        let checkin_id = rca_core::domain::CheckinId(nz(11));
        let problem_id = ProblemId(nz(12));

        let (k1, m1) = format_core_event(&CoreEvent::CheckinDiscovered {
            lesson_id,
            checkin_id,
        });
        assert_eq!(k1, "warning");
        assert!(m1.contains("签到已开启"));

        let (k2, m2) = format_core_event(&CoreEvent::AutoAnswerSubmitted {
            lesson_id,
            problem_id,
        });
        assert_eq!(k2, "success");
        assert!(m2.contains("自动答题完成"));

        let (k3, m3) = format_core_event(&CoreEvent::AutoCheckinSubmitted {
            lesson_id,
            checkin_id,
        });
        assert_eq!(k3, "success");
        assert!(m3.contains("自动签到完成"));
    }

    #[test]
    fn format_event_danmu_call_ppt_slide_and_unlock() {
        let lesson_id = LessonId(nz(20));

        let (k1, m1) = format_core_event(&CoreEvent::DanmuPublished {
            lesson_id,
            user_name: Some("Bob".to_string()),
            content: "hello".to_string(),
        });
        assert_eq!(k1, "info");
        assert!(m1.contains("实时弹幕"));
        assert!(m1.contains("Bob"));
        assert!(m1.contains("hello"));

        let (k1b, m1b) = format_core_event(&CoreEvent::DanmuPublished {
            lesson_id,
            user_name: None,
            content: "hi".to_string(),
        });
        assert_eq!(k1b, "info");
        assert!(m1b.contains("未知"));

        let (k2, m2) = format_core_event(&CoreEvent::CallPaused {
            lesson_id,
            target_name: "Charlie".to_string(),
        });
        assert_eq!(k2, "warning");
        assert!(m2.contains("点名"));
        assert!(m2.contains("Charlie"));

        let (k3, m3) = format_core_event(&CoreEvent::PresentationUpdated {
            lesson_id,
            presentation_id: 99,
        });
        assert_eq!(k3, "info");
        assert!(m3.contains("PPT"));
        assert!(m3.contains("99"));

        let (k4, m4) = format_core_event(&CoreEvent::SlideNavigated {
            lesson_id,
            presentation_id: 99,
            slide_id: 123,
            slide_index: 4,
        });
        assert_eq!(k4, "info");
        assert!(m4.contains("切换幻灯片"));
        assert!(m4.contains("页面 4"));
        assert!(m4.contains("id: 123"));

        let (k5, m5) = format_core_event(&CoreEvent::ProblemUnlocked {
            lesson_id,
            problem_id: ProblemId(nz(77)),
        });
        assert_eq!(k5, "warning");
        assert!(m5.contains("题目已解锁"));
        assert!(m5.contains("77"));
    }
}
