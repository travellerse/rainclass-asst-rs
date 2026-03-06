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
    ) {
        let auto_answer_enabled = cfg.auto_answer_enabled;
        let auto_checkin_enabled = cfg.auto_checkin_enabled;

        match event.0 {
            LessonWsEvent::ProblemPublished { problem } => {
                tracing::info!("收到题目：{}", problem.title);
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
                    "签到开启: lesson={} checkin={}",
                    lesson.lesson_id.0.get(),
                    checkin_id.0.get()
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
                tracing::info!(
                    "Presentation updated: presentation_id={} lesson_id={}",
                    presentation_id,
                    lesson.lesson_id.0.get()
                );
                let _ = event.1.send(CoreEvent::PresentationUpdated {
                    lesson_id: lesson.lesson_id,
                    presentation_id,
                });
            }
            LessonWsEvent::CallPaused { target_name } => {
                tracing::info!(
                    "Roll-call initiated: target={} lesson_id={}",
                    target_name,
                    lesson.lesson_id.0.get()
                );
                let _ = event.1.send(CoreEvent::CallPaused {
                    lesson_id: lesson.lesson_id,
                    target_name,
                });
            }
            LessonWsEvent::DanmuPublished { user_name, content } => {
                tracing::info!(
                    "Danmu received: sender={:?} content={:?} lesson_id={}",
                    user_name,
                    content,
                    lesson.lesson_id.0.get()
                );

                if cfg.auto_danmu_enabled
                    && state
                        .danmu_tracker
                        .track_and_decide(&content, cfg.danmu_threshold, 60, 60)
                {
                    tracing::info!("Auto-replying to danmu: {:?}", content);
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
                tracing::info!("Lesson ended: lesson_id={}", lesson.lesson_id.0.get());
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
                            for problem in history_problems {
                                Self::process_lesson_ws_event(
                                    &api_for_lesson,
                                    &session_for_lesson,
                                    &lesson_clone,
                                    &mut state,
                                    (crate::app::ports::LessonWsEvent::ProblemPublished { problem }, &event_tx_lesson),
                                    &cfg_for_lesson,
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
