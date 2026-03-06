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

    fn default_answer_payload(problem: &Problem) -> Option<AnswerPayload> {
        match problem.problem_type {
            ProblemType::SingleChoice => {
                problem.options.first().map(|option| AnswerPayload::Single {
                    option_id: option.option_id.clone(),
                })
            }
            ProblemType::MultipleChoice => {
                problem
                    .options
                    .first()
                    .map(|option| AnswerPayload::Multiple {
                        option_ids: vec![option.option_id.clone()],
                    })
            }
            ProblemType::FillBlank => Some(AnswerPayload::FillBlank {
                text: String::new(),
            }),
            ProblemType::Unknown => None,
        }
    }

    async fn process_lesson_ws_event(
        api: &Arc<dyn ApiPort>,
        session: &AuthSession,
        lesson: &crate::domain::Lesson,
        answered_problems: &mut HashSet<u64>,
        checked_checkins: &mut HashSet<u64>,
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
                    && answered_problems.insert(problem.problem_id.0.get())
                    && let Some(payload) = Self::default_answer_payload(&problem)
                {
                    match api
                        .submit_answer(session, lesson.lesson_id, problem.problem_id, payload)
                        .await
                    {
                        Ok(()) => {
                            let _ = event.1.send(CoreEvent::AutoAnswerSubmitted {
                                lesson_id: lesson.lesson_id,
                                problem_id: problem.problem_id,
                            });
                        }
                        Err(err) => {
                            let _ = event.1.send(CoreEvent::Error {
                                code: "AUTO_ANSWER_FAILED",
                                message: err.to_string(),
                            });
                        }
                    }
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

                if auto_checkin_enabled && checked_checkins.insert(checkin_id.0.get()) {
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
                    let mut answered_problems = HashSet::new();
                    let mut checked_checkins = HashSet::new();

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
                                    &mut answered_problems,
                                    &mut checked_checkins,
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
                                        &mut answered_problems,
                                        &mut checked_checkins,
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
