# RainClassroomAssistant Rust 详细设计（结构体与函数签名）

## 1. 说明

本文档给出**实现级接口契约**，目标是：

- 直接指导代码落地，避免“简化伪代码”
- 明确 crate 间可见性、依赖与错误边界
- 保证可测试、可替换、可扩展

约定：

- Rust edition: 2021（后续可迁移到 2024）
- 异步运行时：`tokio`
- 错误分层：库内 `thiserror`，应用聚合层可用 `anyhow`
- 日志：`tracing`

---

## 2. workspace 与模块布局

```text
apps/
  rca-desktop/
    src/
      main.rs
      app_controller.rs
      view_model.rs
      ui_binding.rs
crates/
    rca-core/
    src/
      lib.rs
            domain/
                mod.rs
                ids.rs
                lesson.rs
                problem.rs
                checkin.rs
                events.rs
                policy.rs
                errors.rs
            auth/
                mod.rs
                service.rs
                state_machine.rs
                errors.rs
            monitor/
                mod.rs
                engine.rs
                supervisor.rs
                scheduler.rs
                errors.rs
            app/
                mod.rs
                command.rs
                query.rs
                usecases.rs
                event_bus.rs
                app_state.rs
                errors.rs
    rca-infra/
    src/
      lib.rs
            api/
                mod.rs
                client.rs
                auth_api.rs
                lesson_api.rs
                problem_api.rs
                checkin_api.rs
                ws.rs
                dto.rs
                errors.rs
            storage/
                mod.rs
                config_repo.rs
                credential_store.rs
                session_repo.rs
                fs_impl.rs
                keyring_impl.rs
                errors.rs
            notify/
                mod.rs
                notifier.rs
                platform.rs
                errors.rs
            update/
                mod.rs
                checker.rs
                errors.rs
    rca-shared/   # 可选，仅在跨层公共类型显著增多时引入
```

旧拆分映射：

- `rca-domain` + `rca-auth` + `rca-monitor` + `rca-app` => `rca-core`
- `rca-api` + `rca-storage` + `rca-notify` + `update-check` => `rca-infra`

---

## 3. `rca-core::domain` 设计

## 3.1 标识与值对象

```rust
use std::num::NonZeroU64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserId(pub NonZeroU64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CourseId(pub NonZeroU64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LessonId(pub NonZeroU64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProblemId(pub NonZeroU64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CheckinId(pub NonZeroU64);
```

## 3.2 课程与题目模型

```rust
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LessonStatus {
    Scheduled,
    Running,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lesson {
    pub lesson_id: LessonId,
    pub course_id: CourseId,
    pub course_name: String,
    pub teacher_name: String,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub status: LessonStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemType {
    SingleChoice,
    MultipleChoice,
    FillBlank,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProblemOption {
    pub option_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub lesson_id: LessonId,
    pub problem_id: ProblemId,
    pub problem_type: ProblemType,
    pub title: String,
    pub options: Vec<ProblemOption>,
    pub published_at: DateTime<Utc>,
    pub deadline_at: Option<DateTime<Utc>>,
}
```

## 3.3 答案与策略

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerPayload {
    Single { option_id: String },
    Multiple { option_ids: Vec<String> },
    FillBlank { text: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerDecision {
    pub problem_id: ProblemId,
    pub payload: AnswerPayload,
    pub confidence: f32,
    pub source: AnswerSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerSource {
    Heuristic,
    RuleBased,
    UserPreset,
}

pub trait AnswerPolicy: Send + Sync {
    fn decide(&self, problem: &Problem) -> Result<AnswerDecision, DomainError>;
}
```

## 3.4 事件模型

```rust
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TraceId(pub Uuid);

#[derive(Debug, Clone)]
pub struct EventMeta {
    pub trace_id: TraceId,
    pub occurred_at: DateTime<Utc>,
    pub source: &'static str,
}

#[derive(Debug, Clone)]
pub enum DomainEvent {
    LessonStarted { meta: EventMeta, lesson: Lesson },
    LessonEnded { meta: EventMeta, lesson_id: LessonId },
    ProblemPublished { meta: EventMeta, problem: Problem },
    CheckinOpened { meta: EventMeta, lesson_id: LessonId, checkin_id: CheckinId },
}
```

## 3.5 领域错误

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("invalid lesson status transition: {from:?} -> {to:?}")]
    InvalidLessonStatusTransition { from: LessonStatus, to: LessonStatus },

    #[error("problem payload does not match problem type: {problem_id:?}")]
    ProblemPayloadMismatch { problem_id: ProblemId },

    #[error("invalid value: {0}")]
    InvalidValue(String),
}
```

---

## 4. `rca-infra::api` 设计

## 4.1 客户端配置与上下文

```rust
use std::time::Duration;
use reqwest::Url;

#[derive(Debug, Clone)]
pub struct ApiClientConfig {
    pub base_url: Url,
    pub ws_url: Url,
    pub user_agent: String,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub max_retries: u32,
}

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub user_id: u64,
}
```

## 4.2 DTO 与协议接口

```rust
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct OnLessonDto {
    pub lesson_id: u64,
    pub course_id: u64,
    pub course_name: String,
    pub teacher_name: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct ProblemDto {
    pub lesson_id: u64,
    pub problem_id: u64,
    pub problem_type: String,
    pub title: String,
    pub options: Vec<(String, String)>,
    pub published_at: DateTime<Utc>,
    pub deadline_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct CheckinDto {
    pub lesson_id: u64,
    pub checkin_id: u64,
    pub opened_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum WsEventDto {
    ProblemPublished(ProblemDto),
    CheckinOpened(CheckinDto),
    LessonEnded { lesson_id: u64 },
    Unknown { raw_type: String, raw_payload: String },
}

#[async_trait]
pub trait RainClassroomApi: Send + Sync {
    async fn get_on_lessons(&self, auth: &AuthContext) -> Result<Vec<OnLessonDto>, ApiError>;

    async fn get_lesson_problems(
        &self,
        auth: &AuthContext,
        lesson_id: u64,
    ) -> Result<Vec<ProblemDto>, ApiError>;

    async fn submit_answer(
        &self,
        auth: &AuthContext,
        lesson_id: u64,
        problem_id: u64,
        payload: ApiAnswerPayload,
    ) -> Result<ApiSubmitAnswerResult, ApiError>;

    async fn submit_checkin(
        &self,
        auth: &AuthContext,
        lesson_id: u64,
        checkin_id: u64,
    ) -> Result<ApiCheckinResult, ApiError>;

    async fn start_qr_login(&self) -> Result<QrLoginSession, ApiError>;

    async fn poll_qr_login(
        &self,
        session: &QrLoginSession,
    ) -> Result<QrLoginPollResult, ApiError>;

    async fn refresh_session(&self, refresh_token: &str) -> Result<AuthContext, ApiError>;
}
```

## 4.3 WS 抽象

```rust
use futures_core::Stream;
use std::pin::Pin;

pub type WsEventStream = Pin<Box<dyn Stream<Item = Result<WsEventDto, ApiError>> + Send>>;

#[async_trait]
pub trait RainClassroomWs: Send + Sync {
    async fn connect_lesson_stream(
        &self,
        auth: &AuthContext,
        lesson_id: u64,
    ) -> Result<WsEventStream, ApiError>;
}
```

## 4.4 登录 DTO

```rust
use reqwest::Url;

#[derive(Debug, Clone)]
pub struct QrLoginSession {
    pub scene_id: String,
    pub token: String,
    pub qr_url: Url,
    pub expires_at_unix_ms: i64,
}

#[derive(Debug, Clone)]
pub enum QrLoginPollResult {
    Pending,
    Confirmed(AuthContext),
    Expired,
    Rejected,
}
```

> 实现状态（2026-02-28）：`YktApiPort` 已接入 `wss://<host>/wsapp/` 的 `requestlogin/loginsuccess` 基础链路，
> 并通过 `poll_qr_login(scene_id)` 返回 `Pending/Confirmed/Expired/Rejected`。
> `get_lesson_problems` 已实现为 `lesson/basic-info -> presentation/fetch -> slides[].problem` 的解析链路（对齐旧版思路，非 WS 驱动）。
> `RainClassroomWs::connect_lesson_stream` 已在 `YktApiPort` 实现最小课堂流：`checkin -> hello -> wsapp`，并映射 `unlockproblem/probleminfo/lessonfinished` 到 `WsEventDto`。
> 当前该课堂 WS 流已接入 `rca-core` 的监控编排主路径：`StartMonitor` 会为进行中课程建立 `connect_lesson_stream`，并基于 WS 事件触发自动签到/答题。
> 当前 `scene_id` 为客户端生成的会话标识，尚未实现跨进程恢复与二维码图像渲染（仅返回 ticket URL 字符串）。

## 4.5 API 错误

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("websocket error: {0}")]
    WebSocket(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("unauthorized")]
    Unauthorized,

    #[error("rate limited")]
    RateLimited,

    #[error("remote protocol changed: {0}")]
    ProtocolChanged(String),

    #[error("timeout")]
    Timeout,

    #[error("unexpected status: {status}, body: {body}")]
    UnexpectedStatus { status: u16, body: String },
}
```

---

## 5. `rca-infra::storage` 设计

## 5.1 配置模型

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub monitor_interval_secs: u64,
    pub auto_checkin_enabled: bool,
    pub auto_answer_enabled: bool,
    pub answer_delay_ms: u64,
    pub notify_enabled: bool,
    pub check_update_on_startup: bool,
    pub active_tenant: TenantKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TenantKind {
    Rain,
    Hetang,
    Yangtze,
    YellowRiver,
}
```

## 5.2 仓储接口

```rust
use async_trait::async_trait;

#[async_trait]
pub trait ConfigRepository: Send + Sync {
    async fn load(&self) -> Result<AppConfig, StorageError>;
    async fn save(&self, config: &AppConfig) -> Result<(), StorageError>;
}

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub user_id: u64,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_unix_ms: Option<i64>,
}

#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn load(&self) -> Result<Option<SessionRecord>, StorageError>;
    async fn save(&self, session: &SessionRecord) -> Result<(), StorageError>;
    async fn clear(&self) -> Result<(), StorageError>;
}

#[async_trait]
pub trait CredentialStore: Send + Sync {
    async fn save_token_pair(&self, service: &str, account: &str, access: &str, refresh: Option<&str>)
        -> Result<(), StorageError>;

    async fn load_token_pair(&self, service: &str, account: &str)
        -> Result<Option<(String, Option<String>)>, StorageError>;

    async fn delete_token_pair(&self, service: &str, account: &str)
        -> Result<(), StorageError>;
}
```

## 5.3 存储错误

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialize error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("keyring error: {0}")]
    Keyring(String),

    #[error("invalid config: {0}")]
    InvalidConfig(String),
}
```

---

## 6. `rca-core::auth` 设计

## 6.1 登录状态机

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthState {
    LoggedOut,
    WaitingQrScan { scene_id: String, token: String },
    WaitingConfirm { scene_id: String },
    LoggedIn { user_id: u64 },
    Refreshing { user_id: u64 },
    Failed { reason: String },
}
```

## 6.2 服务接口

```rust
use async_trait::async_trait;

#[async_trait]
pub trait AuthService: Send + Sync {
    async fn current_state(&self) -> AuthState;

    async fn begin_qr_login(&self) -> Result<QrLoginBootstrap, AuthError>;

    async fn poll_qr_login(&self, scene_id: &str) -> Result<QrLoginProgress, AuthError>;

    async fn restore_session(&self) -> Result<Option<AuthSession>, AuthError>;

    async fn refresh_if_needed(&self) -> Result<AuthSession, AuthError>;

    async fn logout(&self) -> Result<(), AuthError>;
}

#[derive(Debug, Clone)]
pub struct QrLoginBootstrap {
    pub scene_id: String,
    pub token: String,
    pub qr_svg: String,
}

#[derive(Debug, Clone)]
pub enum QrLoginProgress {
    Pending,
    Confirmed(AuthSession),
    Expired,
    Rejected,
}

#[derive(Debug, Clone)]
pub struct AuthSession {
    pub user_id: u64,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_unix_ms: Option<i64>,
}
```

## 6.3 认证错误

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("api error: {0}")]
    Api(#[from] rca_infra::api::ApiError),

    #[error("storage error: {0}")]
    Storage(#[from] rca_infra::storage::StorageError),

    #[error("session missing")]
    SessionMissing,

    #[error("session expired")]
    SessionExpired,

    #[error("invalid auth state: {0}")]
    InvalidState(String),
}
```

---

## 7. `rca-core::monitor` 设计

## 7.1 引擎配置与句柄

```rust
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct MonitorConfig {
    pub poll_interval: Duration,
    pub ws_reconnect_backoff_base: Duration,
    pub ws_reconnect_backoff_max: Duration,
    pub max_parallel_lessons: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MonitorTaskId(pub u64);

#[derive(Debug)]
pub struct MonitorHandle {
    pub task_id: MonitorTaskId,
}
```

## 7.2 事件与命令

```rust
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub enum CoreEvent {
    MonitorStarted { at: DateTime<Utc> },
    MonitorStopped { at: DateTime<Utc> },
    LessonDiscovered { lesson: rca_core::domain::Lesson },
    ProblemDiscovered { problem: rca_core::domain::Problem },
    CheckinDiscovered { lesson_id: rca_core::domain::LessonId, checkin_id: rca_core::domain::CheckinId },
    AutoAnswerSubmitted { lesson_id: rca_core::domain::LessonId, problem_id: rca_core::domain::ProblemId },
    AutoCheckinSubmitted { lesson_id: rca_core::domain::LessonId, checkin_id: rca_core::domain::CheckinId },
    Warning { code: &'static str, message: String },
    Error { code: &'static str, message: String },
}

#[derive(Debug, Clone)]
pub enum MonitorCommand {
    Start,
    Stop,
    ReloadConfig,
    ForceSync,
}
```

## 7.3 引擎接口

```rust
use async_trait::async_trait;
use tokio::sync::mpsc;

#[async_trait]
pub trait MonitorEngine: Send + Sync {
    async fn start(
        &self,
        session: rca_core::auth::AuthSession,
        cfg: MonitorConfig,
    ) -> Result<MonitorHandle, MonitorError>;

    async fn stop(&self, handle: MonitorHandle) -> Result<(), MonitorError>;

    fn subscribe_events(&self) -> mpsc::Receiver<CoreEvent>;

    async fn send_command(&self, command: MonitorCommand) -> Result<(), MonitorError>;
}
```

> 实现状态（2026-02-28）：当前已在 `CoreAppService` 内实现最小监控闭环：
> `StartMonitor` 现已切换为 WS 事件驱动：启动时获取进行中课程并为每门课连接 `connect_lesson_stream`，
> 后续通过 `ProblemPublished/CheckinOpened` 事件触发自动答题/自动签到（不再依赖周期轮询题目与签到），
> `StopMonitor/Logout` 会停止该后台任务；课堂 WS 断开/连接失败时会按固定退避自动重连。
> 已新增 `RefreshSession` 命令链路（优先 refresh_token，缺失时回退使用当前 access token 做 cookie 会话校验），
> 并在桌面端启动序列中执行 `LoadConfig -> RestoreSession -> RefreshSession -> CheckUpdate(可配置)`。
> `MonitorEngine` trait 仍为后续独立引擎抽象预留。

> 存储实现状态（2026-02-28）：`CoreSessionStoreAdapter` 已切换为 keyring 主路径，
> JSON 会话文件仅保留非敏感元数据（并支持旧明文会话的首次读取迁移）。

> 通知实现状态（2026-02-28）：自动签到与自动答题成功会在 `notify_enabled=true` 时触发基础通知（并发送 `AppEvent::Notification`）。

## 7.4 监控错误

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MonitorError {
    #[error("auth error: {0}")]
    Auth(#[from] rca_core::auth::AuthError),

    #[error("api error: {0}")]
    Api(#[from] rca_infra::api::ApiError),

    #[error("domain error: {0}")]
    Domain(#[from] rca_core::domain::DomainError),

    #[error("internal channel closed")]
    ChannelClosed,

    #[error("task join error: {0}")]
    Join(String),

    #[error("already started")]
    AlreadyStarted,

    #[error("not running")]
    NotRunning,
}
```

---

## 8. `rca-infra::notify` 设计

## 8.1 通知模型与接口

```rust
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub enum NotifyLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: String,
    pub title: String,
    pub body: String,
    pub level: NotifyLevel,
    pub created_at: DateTime<Utc>,
}

#[async_trait::async_trait]
pub trait Notifier: Send + Sync {
    async fn notify(&self, msg: Notification) -> Result<(), NotifyError>;
}
```

## 8.2 通知错误

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NotifyError {
    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),

    #[error("platform error: {0}")]
    Platform(String),
}
```

---

## 9. `rca-core::app`（应用层）设计

## 9.1 命令、查询、视图状态

```rust
#[derive(Debug, Clone)]
pub enum AppCommand {
    LoadConfig,
    RestoreSession,
    RefreshSession,
    LoginByQr,
    PollLogin { scene_id: String },
    Logout,
    StartMonitor,
    StopMonitor,
    CheckUpdate,
    SaveConfig { config: AppConfigDto },
}

#[derive(Debug, Clone)]
pub enum AppQuery {
    GetAppState,
    GetConfig,
    GetRecentEvents { limit: usize },
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub auth_state: rca_core::auth::AuthState,
    pub monitor_running: bool,
    pub current_lessons: Vec<rca_core::domain::Lesson>,
    pub recent_events: Vec<rca_core::monitor::CoreEvent>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    StateChanged(AppState),
    Notification(rca_infra::notify::Notification),
    UpdateAvailable { version: String, url: String },
}
```

## 9.2 应用服务接口

```rust
use async_trait::async_trait;
use tokio::sync::mpsc;

#[async_trait]
pub trait AppService: Send + Sync {
    async fn handle_command(&self, cmd: AppCommand) -> Result<(), AppError>;

    async fn handle_query(&self, query: AppQuery) -> Result<AppQueryResult, AppError>;

    fn subscribe_events(&self) -> mpsc::Receiver<AppEvent>;
}

#[derive(Debug, Clone)]
pub enum AppQueryResult {
    State(AppState),
    Config(rca_infra::storage::AppConfig),
    Events(Vec<rca_core::monitor::CoreEvent>),
}
```

## 9.3 更新检查接口

```rust
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub latest_version: String,
    pub release_url: String,
    pub published_at_unix_ms: i64,
}

#[async_trait]
pub trait UpdateChecker: Send + Sync {
    async fn check_latest(&self, current_version: &str) -> Result<Option<UpdateInfo>, AppError>;
}
```

## 9.4 应用错误

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("auth error: {0}")]
    Auth(#[from] rca_core::auth::AuthError),

    #[error("monitor error: {0}")]
    Monitor(#[from] rca_core::monitor::MonitorError),

    #[error("storage error: {0}")]
    Storage(#[from] rca_infra::storage::StorageError),

    #[error("notify error: {0}")]
    Notify(#[from] rca_infra::notify::NotifyError),

    #[error("invalid command: {0}")]
    InvalidCommand(String),

    #[error("runtime error: {0}")]
    Runtime(String),
}
```

---

## 10. `apps/rca-desktop`（Slint 应用）设计

## 10.1 ViewModel

```rust
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct MainViewModel {
    pub auth_status_text: String,
    pub monitor_status_text: String,
    pub current_course_count: i32,
    pub last_error_text: String,
}

pub struct AppController {
    app: Arc<dyn rca_core::app::AppService>,
}

impl AppController {
    pub fn new(app: Arc<dyn rca_core::app::AppService>) -> Self;

    pub async fn on_login_clicked(&self) -> Result<(), rca_core::app::AppError>;

    pub async fn on_start_monitor_clicked(&self) -> Result<(), rca_core::app::AppError>;

    pub async fn on_stop_monitor_clicked(&self) -> Result<(), rca_core::app::AppError>;

    pub async fn on_save_settings(&self, cfg: rca_infra::storage::AppConfig) -> Result<(), rca_core::app::AppError>;

    pub async fn on_check_update(&self) -> Result<(), rca_core::app::AppError>;
}
```

## 10.2 UI 绑定边界

```rust
pub trait UiBinder {
    fn bind_callbacks(&self);
    fn render_state(&self, vm: &MainViewModel);
    fn show_error(&self, message: &str);
}
```

---

## 11. 关键实现约束（必须执行）

1. **禁止在 UI 回调中直接发网络请求**，统一经 `AppService::handle_command`。
2. **禁止在 core crate 使用全局可变单例**，并发状态必须可追踪。
3. **所有 public API 必须有错误语义**，不允许 `Result<T, String>`。
4. **token 不允许落盘明文**，必须经 `CredentialStore`。
5. **所有可重试网络调用必须可观测**（日志含 trace_id、重试次数、耗时）。

> 实现状态（2026-02-28）：`rca-core::app::ports` 已完成第一批错误收敛，
> `ApiPort/SessionStorePort/ConfigStorePort/NotifierPort/UpdateCheckerPort` 已从 `Result<_, String>`
> 迁移到 typed port error（`ApiPortError/StoragePortError/NotifyPortError/UpdatePortError`）。
> 第二批已完成应用层聚合：`AppError` 新增端口错误结构化变体并通过 `From` 链路接入 `CoreAppService`。
> 其余模块（如 `auth/monitor` 子域与更细粒度 infra 错误枚举）仍在后续批次中。
> 当前 workspace 代码中的 `Result<_, String>` 已清零。
> 本轮补齐后，`rca-core` 中同类“泛化字符串包装错误”（如 `Auth(String)`/`Message(String)`）也已完成语义化收敛。

---

## 12. 测试签名（建议）

```rust
#[tokio::test]
async fn auth_should_restore_session_from_keyring() -> anyhow::Result<()>;

#[tokio::test]
async fn monitor_should_emit_problem_discovered_event() -> anyhow::Result<()>;

#[tokio::test]
async fn app_should_submit_checkin_on_checkin_opened_event() -> anyhow::Result<()>;

#[tokio::test]
async fn app_should_not_panic_when_ws_temporarily_disconnected() -> anyhow::Result<()>;
```

---

## 13. 依赖建议（按 crate）

- `rca-core`: `chrono`, `uuid`, `tokio`, `async-trait`, `thiserror`, `tracing`, `futures`, `backoff`
- `rca-infra`: `reqwest`, `tokio`, `tokio-tungstenite`, `serde`, `serde_json`, `directories`, `keyring`, `notify-rust`, `thiserror`, `url`, `async-trait`
- `apps/rca-desktop`: `slint`, `tokio`, `tracing`
- `rca-shared`（可选）: `serde`, `thiserror`

---

## 14. 版本化策略

- 所有 crate 从 `0.x` 开始，采用语义化版本。
- 非兼容 API 改动必须：
  1) 提升次版本（`0.x` 的 `x`），
  2) 更新本文件签名定义，
  3) 增加迁移说明。
