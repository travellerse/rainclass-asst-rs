use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Duration, sleep};

use crate::app::ports::{ApiPort, LessonWsEvent};
use crate::auth::AuthSession;
use crate::domain::{AnswerPayload, Problem, ProblemType};
use crate::monitor::{
    CoreEvent, MonitorConfig, MonitorEngine, MonitorError, MonitorHandle, MonitorTaskId,
};

pub struct CoreMonitorEngine {
    api: Arc<dyn ApiPort>,
    state: Arc<Mutex<EngineState>>,
}

#[derive(Debug, Clone, Copy)]
enum ProblemSource {
    Ppt,
    Ws,
}

struct EngineState {
    event_tx: tokio::sync::broadcast::Sender<CoreEvent>,
    runtime: Option<EngineRuntime>,
}

struct EngineRuntime {
    stop_tx: watch::Sender<bool>,
    join_handle: JoinHandle<()>,
}

struct LessonState {
    answered_problems: HashSet<u64>,
    checked_checkins: HashSet<u64>,
    danmu_tracker: crate::monitor::DanmuTracker,
    current_presentation_id: Option<u64>,
}

impl CoreMonitorEngine {
    pub fn new(api: Arc<dyn ApiPort>) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(128);
        Self {
            api,
            state: Arc::new(Mutex::new(EngineState {
                event_tx,
                runtime: None,
            })),
        }
    }

    /// Resolve the best answer payload using extracted correct answers.
    /// If `allow_random_guess` is true, falls back to heuristic (first option) when no correct answers are available.
    /// Otherwise, returns None.
    fn resolve_answer_payload(
        problem: &Problem,
        allow_random_guess: bool,
    ) -> Option<AnswerPayload> {
        match problem.problem_type {
            ProblemType::SingleChoice => {
                // Prefer correct answer from slide data
                if let Some(answer_id) = problem.correct_answers.first() {
                    Some(AnswerPayload::Single {
                        option_id: answer_id.clone(),
                    })
                } else if allow_random_guess {
                    // Fallback: pick first option
                    problem.options.first().map(|opt| AnswerPayload::Single {
                        option_id: opt.option_id.clone(),
                    })
                } else {
                    None
                }
            }
            ProblemType::MultipleChoice => {
                if !problem.correct_answers.is_empty() {
                    Some(AnswerPayload::Multiple {
                        option_ids: problem.correct_answers.clone(),
                    })
                } else if allow_random_guess {
                    // Fallback: pick first option
                    problem.options.first().map(|opt| AnswerPayload::Multiple {
                        option_ids: vec![opt.option_id.clone()],
                    })
                } else {
                    None
                }
            }
            ProblemType::FillBlank => {
                // Use blanks data: pick first accepted value of each blank,
                // join with "," for multi-blank problems
                if !problem.blanks.is_empty() {
                    let text = problem
                        .blanks
                        .iter()
                        .filter_map(|blank| blank.accepted_values.first().cloned())
                        .collect::<Vec<_>>()
                        .join(",");
                    Some(AnswerPayload::FillBlank { text })
                } else if allow_random_guess {
                    Some(AnswerPayload::FillBlank {
                        text: String::new(),
                    })
                } else {
                    None
                }
            }
            ProblemType::Unknown => None,
        }
    }

    async fn process_lesson_ws_event(
        api: &Arc<dyn ApiPort>,
        session: &AuthSession,
        lesson: &crate::domain::Lesson,
        state: &mut LessonState,
        event: (LessonWsEvent, &tokio::sync::broadcast::Sender<CoreEvent>),
        cfg: &MonitorConfig,
        source: ProblemSource,
    ) {
        let auto_answer_enabled = cfg.auto_answer_enabled;
        let auto_checkin_enabled = cfg.auto_checkin_enabled;

        match event.0 {
            LessonWsEvent::ProblemPublished { problem } => {
                tracing::info!(
                    lesson_id = lesson.lesson_id.0.get(),
                    problem_id = problem.problem_id.0.get(),
                    source = ?source,
                    problem_type = ?problem.problem_type,
                    title = %problem.title,
                    correct_answers = ?problem.correct_answers,
                    blanks = ?problem
                        .blanks
                        .iter()
                        .map(|b| b.accepted_values.clone())
                        .collect::<Vec<_>>(),
                    "problem discovered"
                );
                let _ = event.1.send(CoreEvent::ProblemDiscovered {
                    problem: problem.clone(),
                });

                if auto_answer_enabled
                    && state.answered_problems.insert(problem.problem_id.0.get())
                    && let Some(payload) =
                        Self::resolve_answer_payload(&problem, cfg.auto_answer_random_guess)
                {
                    let delay = crate::monitor::calculate_wait_time(
                        problem.limit_secs,
                        &cfg.delay_strategy,
                    );

                    let api_clone = api.clone();
                    let session_clone = session.clone();
                    let lesson_id = lesson.lesson_id;
                    let problem_id = problem.problem_id;
                    let sender = event.1.clone();
                    let delay_strategy = cfg.delay_strategy;

                    tokio::spawn(async move {
                        if delay > Duration::ZERO {
                            tracing::info!(
                                lesson_id = lesson_id.0.get(),
                                problem_id = problem_id.0.get(),
                                "等待 {:?} 后提交答案 (策略: {:?})",
                                delay,
                                delay_strategy
                            );
                            sleep(delay).await;
                        }

                        match api_clone
                            .submit_answer(&session_clone, lesson_id, problem_id, payload)
                            .await
                        {
                            Ok(()) => {
                                let _ = sender.send(CoreEvent::AutoAnswerSubmitted {
                                    lesson_id,
                                    problem_id,
                                });
                            }
                            Err(err) => {
                                let _ = sender.send(CoreEvent::Error {
                                    code: "AUTO_ANSWER_FAILED",
                                    message: err.to_string(),
                                });
                            }
                        }
                    });
                }
            }
            LessonWsEvent::CheckinOpened { checkin_id } => {
                tracing::info!(
                    lesson_id = lesson.lesson_id.0.get(),
                    checkin_id = checkin_id.0.get(),
                    "checkin opened"
                );
                let _ = event.1.send(CoreEvent::CheckinDiscovered {
                    lesson_id: lesson.lesson_id,
                    checkin_id,
                });

                if auto_checkin_enabled && state.checked_checkins.insert(checkin_id.0.get()) {
                    match api
                        .submit_checkin(session, lesson.lesson_id, checkin_id)
                        .await
                    {
                        Ok(()) => {
                            let _ = event.1.send(CoreEvent::AutoCheckinSubmitted {
                                lesson_id: lesson.lesson_id,
                                checkin_id,
                            });
                        }
                        Err(err) => {
                            let _ = event.1.send(CoreEvent::Error {
                                code: "AUTO_CHECKIN_FAILED",
                                message: err.to_string(),
                            });
                        }
                    }
                }
            }
            LessonWsEvent::PresentationUpdated { presentation_id } => {
                // INFO should be user-facing; keep internal diff details out of INFO.
                tracing::info!(
                    lesson_id = lesson.lesson_id.0.get(),
                    presentation_id = presentation_id,
                    "检测到新的 PPT"
                );
                state.current_presentation_id = Some(presentation_id);
                let _ = event.1.send(CoreEvent::PresentationUpdated {
                    lesson_id: lesson.lesson_id,
                    presentation_id,
                });
            }
            LessonWsEvent::SlideNavigated {
                presentation_id,
                slide_id,
                slide_index,
            } => {
                state.current_presentation_id = Some(presentation_id);
                tracing::info!(
                    lesson_id = lesson.lesson_id.0.get(),
                    presentation_id = presentation_id,
                    slide_index = slide_index,
                    slide_id = slide_id,
                    "slide navigated"
                );
                let _ = event.1.send(CoreEvent::SlideNavigated {
                    lesson_id: lesson.lesson_id,
                    presentation_id,
                    slide_id,
                    slide_index,
                });
            }
            LessonWsEvent::ProblemUnlocked { problem_id } => {
                tracing::info!(
                    lesson_id = lesson.lesson_id.0.get(),
                    problem_id = problem_id.0.get(),
                    "problem unlocked"
                );
                let _ = event.1.send(CoreEvent::ProblemUnlocked {
                    lesson_id: lesson.lesson_id,
                    problem_id,
                });
            }
            LessonWsEvent::CallPaused { target_name } => {
                tracing::info!(
                    lesson_id = lesson.lesson_id.0.get(),
                    target_name = %target_name,
                    "call paused"
                );
                let _ = event.1.send(CoreEvent::CallPaused {
                    lesson_id: lesson.lesson_id,
                    target_name,
                });
            }
            LessonWsEvent::DanmuPublished { user_name, content } => {
                tracing::info!(
                    lesson_id = lesson.lesson_id.0.get(),
                    sender = ?user_name,
                    content = %content,
                    "danmu received"
                );

                if cfg.auto_danmu_enabled
                    && state
                        .danmu_tracker
                        .track_and_decide(&content, cfg.danmu_threshold, 60, 60)
                {
                    tracing::info!(
                        lesson_id = lesson.lesson_id.0.get(),
                        "auto replying to danmu"
                    );
                    let content_clone = content.clone();
                    let session_clone = session.clone();
                    let lesson_id = lesson.lesson_id;
                    let api_clone = api.clone();
                    let event_tx_clone = event.1.clone();

                    tokio::spawn(async move {
                        if let Err(e) = api_clone
                            .send_danmu(&session_clone, lesson_id, &content_clone)
                            .await
                        {
                            let _ = event_tx_clone.send(CoreEvent::Error {
                                code: "AUTO_DANMU_FAILED",
                                message: e.to_string(),
                            });
                        }
                    });
                }

                let _ = event.1.send(CoreEvent::DanmuPublished {
                    lesson_id: lesson.lesson_id,
                    user_name,
                    content,
                });
            }
            LessonWsEvent::LessonEnded => {
                tracing::info!(lesson_id = lesson.lesson_id.0.get(), "lesson ended");
                let _ = event.1.send(CoreEvent::MonitorStopped {
                    at: chrono::Utc::now(),
                });
            }
            LessonWsEvent::Warning { message } => {
                tracing::warn!("Warning received: {}", message);
                let _ = event.1.send(CoreEvent::Warning {
                    code: "WS_WARNING",
                    message,
                });
            }
            LessonWsEvent::Unknown { raw_type } => {
                if raw_type == "showfinished" {
                    tracing::warn!(
                        lesson_id = lesson.lesson_id.0.get(),
                        op = %raw_type,
                        "unhandled ws op"
                    );
                } else {
                    tracing::trace!(
                        lesson_id = lesson.lesson_id.0.get(),
                        op = %raw_type,
                        "ignored unknown ws op"
                    );
                }
            }
        }
    }
}

#[async_trait]
impl MonitorEngine for CoreMonitorEngine {
    async fn start(
        &self,
        session: AuthSession,
        cfg: MonitorConfig,
    ) -> Result<MonitorHandle, MonitorError> {
        let mut guard = self.state.lock().await;
        if guard.runtime.is_some() {
            return Ok(MonitorHandle {
                task_id: MonitorTaskId(1),
            });
        }

        let event_tx = guard.event_tx.clone();

        let api = self.api.clone();
        let (stop_tx, mut stop_rx) = watch::channel(false);

        let join_handle = tokio::spawn(async move {
            let _ = event_tx.send(CoreEvent::MonitorStarted {
                at: chrono::Utc::now(),
            });

            let lessons = match api.get_on_lessons(&session).await {
                Ok(lessons) => lessons,
                Err(err) => {
                    let _ = event_tx.send(CoreEvent::Error {
                        code: "MONITOR_LESSON_SYNC_FAILED",
                        message: err.to_string(),
                    });
                    return;
                }
            };

            for lesson in &lessons {
                if lesson.teacher_name.is_empty() {
                    tracing::info!("发现课程：{}", lesson.course_name);
                } else {
                    tracing::info!("发现课程：{} ({})", lesson.course_name, lesson.teacher_name);
                }
                let _ = event_tx.send(CoreEvent::LessonDiscovered {
                    lesson: lesson.clone(),
                });
            }

            let mut join_set = JoinSet::new();
            for lesson in lessons {
                let api_for_lesson = api.clone();
                let event_tx_lesson = event_tx.clone();
                let mut stop_rx_lesson = stop_rx.clone();
                let lesson_clone = lesson.clone();
                let session_for_lesson = session.clone();
                let cfg_for_lesson = cfg.clone();

                join_set.spawn(async move {
                    let mut state = LessonState {
                        answered_problems: HashSet::new(),
                        checked_checkins: HashSet::new(),
                        danmu_tracker: crate::monitor::DanmuTracker::new(),
                        current_presentation_id: None,
                    };

                    loop {
                        if *stop_rx_lesson.borrow() {
                            break;
                        }

                        let connect_result = api_for_lesson.connect_lesson_stream(&session_for_lesson, lesson_clone.lesson_id).await;

                        let mut ws_rx = match connect_result {
                            Ok(ws_rx) => ws_rx,
                            Err(err) => {
                                let _ = event_tx_lesson.send(CoreEvent::Error {
                                    code: "MONITOR_WS_CONNECT_FAILED",
                                    message: err.to_string(),
                                });
                                tokio::select! {
                                    _ = stop_rx_lesson.changed() => {
                                        if *stop_rx_lesson.borrow() {
                                            break;
                                        }
                                    }
                                    _ = sleep(Duration::from_secs(2)) => {}
                                }
                                continue;
                            }
                        };

                        if let Ok(history_problems) = api_for_lesson.get_lesson_problems(&session_for_lesson, lesson_clone.lesson_id).await {
                            tracing::debug!(
                                lesson_id = lesson_clone.lesson_id.0.get(),
                                problems = history_problems.len(),
                                "ppt problems loaded"
                            );
                            for problem in history_problems {
                                Self::process_lesson_ws_event(
                                    &api_for_lesson,
                                    &session_for_lesson,
                                    &lesson_clone,
                                    &mut state,
                                    (crate::app::ports::LessonWsEvent::ProblemPublished { problem }, &event_tx_lesson),
                                    &cfg_for_lesson,
                                    ProblemSource::Ppt,
                                ).await;
                            }
                        }

                        loop {
                            tokio::select! {
                                _ = stop_rx_lesson.changed() => {
                                    if *stop_rx_lesson.borrow() {
                                        return;
                                    }
                                }
                                maybe_event = ws_rx.recv() => {
                                    let Some(event) = maybe_event else {
                                        let _ = event_tx_lesson.send(CoreEvent::Warning {
                                            code: "MONITOR_WS_STREAM_CLOSED",
                                            message: format!("lesson ws stream closed: {}", lesson_clone.lesson_id.0.get()),
                                        });
                                        break;
                                    };
                                    Self::process_lesson_ws_event(
                                        &api_for_lesson,
                                        &session_for_lesson,
                                        &lesson_clone,
                                        &mut state,
                                        (event, &event_tx_lesson),
                                        &cfg_for_lesson,
                                        ProblemSource::Ws,
                                    ).await;
                                }
                            }
                        }

                        tokio::select! {
                            _ = stop_rx_lesson.changed() => {
                                if *stop_rx_lesson.borrow() {
                                    break;
                                }
                            }
                            _ = sleep(Duration::from_secs(1)) => {}
                        }
                    }
                });
            }

            loop {
                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            join_set.abort_all();
                            while join_set.join_next().await.is_some() {}
                            break;
                        }
                    }
                    maybe_done = join_set.join_next() => {
                        if maybe_done.is_none() {
                            break;
                        }
                    }
                }
            }

            let _ = event_tx.send(CoreEvent::MonitorStopped {
                at: chrono::Utc::now(),
            });
        });

        guard.runtime = Some(EngineRuntime {
            stop_tx,
            join_handle,
        });

        Ok(MonitorHandle {
            task_id: MonitorTaskId(1),
        })
    }

    async fn stop(&self, _handle: MonitorHandle) -> Result<(), MonitorError> {
        let runtime = {
            let mut guard = self.state.lock().await;
            guard.runtime.take()
        };

        if let Some(runtime) = runtime {
            let _ = runtime.stop_tx.send(true);
            let _ = runtime.join_handle.await;
        }
        Ok(())
    }

    fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<CoreEvent> {
        let guard = self
            .state
            .try_lock()
            .expect("state should not be locked here");
        guard.event_tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use chrono::Utc;

    use crate::domain::*;

    use super::CoreMonitorEngine;

    fn make_problem(
        problem_type: ProblemType,
        options: Vec<ProblemOption>,
        correct_answers: Vec<String>,
        blanks: Vec<BlankAnswer>,
    ) -> Problem {
        Problem {
            lesson_id: LessonId(NonZeroU64::new(1).unwrap()),
            problem_id: ProblemId(NonZeroU64::new(1).unwrap()),
            problem_type,
            title: "test".to_string(),
            options,
            correct_answers,
            blanks,
            limit_secs: Some(60),
            published_at: Utc::now(),
            deadline_at: None,
        }
    }

    fn opts(ids: &[&str]) -> Vec<ProblemOption> {
        ids.iter()
            .map(|id| ProblemOption {
                option_id: id.to_string(),
                text: format!("Option {id}"),
            })
            .collect()
    }

    // ── SingleChoice ───────────────────────────────────────────

    #[test]
    fn single_choice_with_correct_answer() {
        let p = make_problem(
            ProblemType::SingleChoice,
            opts(&["A", "B"]),
            vec!["B".into()],
            vec![],
        );
        let payload = CoreMonitorEngine::resolve_answer_payload(&p, false);
        assert_eq!(
            payload,
            Some(AnswerPayload::Single {
                option_id: "B".into()
            })
        );
    }

    #[test]
    fn single_choice_no_answer_random_guess() {
        let p = make_problem(ProblemType::SingleChoice, opts(&["X", "Y"]), vec![], vec![]);
        let payload = CoreMonitorEngine::resolve_answer_payload(&p, true);
        assert_eq!(
            payload,
            Some(AnswerPayload::Single {
                option_id: "X".into()
            })
        );
    }

    #[test]
    fn single_choice_no_answer_no_guess() {
        let p = make_problem(ProblemType::SingleChoice, opts(&["X"]), vec![], vec![]);
        assert!(CoreMonitorEngine::resolve_answer_payload(&p, false).is_none());
    }

    #[test]
    fn single_choice_no_answer_no_options() {
        let p = make_problem(ProblemType::SingleChoice, vec![], vec![], vec![]);
        assert!(CoreMonitorEngine::resolve_answer_payload(&p, true).is_none());
    }

    // ── MultipleChoice ─────────────────────────────────────────

    #[test]
    fn multiple_choice_with_correct_answers() {
        let p = make_problem(
            ProblemType::MultipleChoice,
            opts(&["A", "B", "C"]),
            vec!["A".into(), "C".into()],
            vec![],
        );
        let payload = CoreMonitorEngine::resolve_answer_payload(&p, false);
        assert_eq!(
            payload,
            Some(AnswerPayload::Multiple {
                option_ids: vec!["A".into(), "C".into()]
            })
        );
    }

    #[test]
    fn multiple_choice_no_answer_random_guess() {
        let p = make_problem(
            ProblemType::MultipleChoice,
            opts(&["A", "B"]),
            vec![],
            vec![],
        );
        let payload = CoreMonitorEngine::resolve_answer_payload(&p, true);
        assert_eq!(
            payload,
            Some(AnswerPayload::Multiple {
                option_ids: vec!["A".into()]
            })
        );
    }

    #[test]
    fn multiple_choice_no_answer_no_guess() {
        let p = make_problem(ProblemType::MultipleChoice, opts(&["A"]), vec![], vec![]);
        assert!(CoreMonitorEngine::resolve_answer_payload(&p, false).is_none());
    }

    // ── FillBlank ──────────────────────────────────────────────

    #[test]
    fn fill_blank_with_blanks() {
        let blanks = vec![
            BlankAnswer {
                accepted_values: vec!["hello".into(), "hi".into()],
            },
            BlankAnswer {
                accepted_values: vec!["world".into()],
            },
        ];
        let p = make_problem(ProblemType::FillBlank, vec![], vec![], blanks);
        let payload = CoreMonitorEngine::resolve_answer_payload(&p, false);
        assert_eq!(
            payload,
            Some(AnswerPayload::FillBlank {
                text: "hello,world".into()
            })
        );
    }

    #[test]
    fn fill_blank_no_blanks_random_guess() {
        let p = make_problem(ProblemType::FillBlank, vec![], vec![], vec![]);
        let payload = CoreMonitorEngine::resolve_answer_payload(&p, true);
        assert_eq!(
            payload,
            Some(AnswerPayload::FillBlank {
                text: String::new()
            })
        );
    }

    #[test]
    fn fill_blank_no_blanks_no_guess() {
        let p = make_problem(ProblemType::FillBlank, vec![], vec![], vec![]);
        assert!(CoreMonitorEngine::resolve_answer_payload(&p, false).is_none());
    }

    // ── Unknown ────────────────────────────────────────────────

    #[test]
    fn unknown_type_always_none() {
        let p = make_problem(ProblemType::Unknown, opts(&["A"]), vec!["A".into()], vec![]);
        assert!(CoreMonitorEngine::resolve_answer_payload(&p, true).is_none());
    }
}
