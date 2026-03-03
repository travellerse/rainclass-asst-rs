# RainClassroomAssistant Rust 重写顶层架构设计（TLD）

## 1. 文档目标与边界

本文档用于定义 Rust + Slint 重写版的**顶层架构**，覆盖：

- 业务域边界与模块职责
- crate 拆分与依赖方向
- 跨平台能力抽象（Linux/Windows/macOS）
- 并发与事件流模型
- 安全、可观测性、发布与质量门禁

不包含逐字段数据结构与函数签名细节；该内容在 `docs/02-detailed-structs-and-signatures.md`。

---

## 2. 设计原则

### 2.1 核心原则

1. **业务核心与 UI 解耦**：Slint 仅承载视图与交互，不直接承载协议逻辑。
2. **可复用优先**：核心能力沉淀为独立 crates，可被 CLI、TUI、Web 或测试驱动复用。
3. **避免重复造轮子**：优先采用成熟 Rust 生态库，围绕抽象层构建适配器。
4. **可测试性优先**：关键逻辑要求“无 UI 可测”。
5. **跨平台一致性**：统一抽象接口，平台差异在适配层处理。
6. **可恢复性与可观测性**：所有长生命周期任务需可取消、可重试、可追踪。

### 2.2 非目标（MVP 阶段）

- 不做应用内二进制自更新（仅检查版本并跳转下载页）。
- 不强制首版对齐 Python 所有边缘功能。
- 不在 UI 层实现复杂流程编排。

---

## 3. 需求与约束映射

### 3.1 已确认产品约束

- 功能范围：MVP（登录 + 课程监控 + 自动签到/答题 + 基础通知）
- 平台范围：Linux / Windows / macOS 三平台同步
- 更新策略：检查新版本并跳转下载
- 凭据策略：系统密钥链（keyring）
- 依赖管理：通过 `cargo` 命令维护依赖，不直接修改依赖清单

### 3.2 工程约束

- 网络协议存在变动风险，需通过协议适配层与回归测试降低风险。
- 桌面能力（通知、托盘、凭据）在三平台可用性不一致，必须具备降级路径。
- GUI 风格目标是现代、扁平、友好，但遵循 Slint 可维护组件化方案。

---

## 4. 目标架构总览

## 4.1 逻辑分层

```text
┌──────────────────────────────────────────────┐
│                 App Shell (Slint)            │
│  - View / ViewModel / Interaction Adapter    │
└──────────────────────┬───────────────────────┘
                       │ UiCommand / UiQuery
┌──────────────────────▼───────────────────────┐
│               Application Layer              │
│  - UseCases / Orchestration / Policy         │
│  - Command Handler / Event Dispatcher        │
└──────────────────────┬───────────────────────┘
                       │ Domain Events / DTO
┌──────────────────────▼───────────────────────┐
│                 Domain Layer                 │
│  - Lesson / Problem / Checkin / Session      │
│  - Business Rules / Deterministic Policies   │
└──────────┬──────────────┬──────────────┬─────┘
           │              │              │
┌──────────▼───────┐ ┌────▼──────────┐ ┌─▼──────────────┐
│ Infrastructure   │ │ Integrations  │ │ Platform Ports │
│ - HTTP/WS Client │ │ - Update Feed │ │ - Keyring      │
│ - Storage/Cache  │ │ - Notification │ │ - Tray/Notify  │
└──────────────────┘ └───────────────┘ └────────────────┘
```

## 4.2 运行时进程模型

- 单进程多任务模型（`tokio` runtime）
- 主线程：Slint UI 事件循环
- 后台任务：监控轮询、WS 接收、通知分发、配置持久化
- 线程间通信：`tokio::sync::mpsc` + `watch` + `oneshot`

---

## 5. crate 拆分设计

建议采用 Cargo workspace，目录示例：

```text
rainclass-asst-rs/
  apps/
    rca-desktop/                # Slint 桌面应用入口
  crates/
    rca-core/                   # 业务核心（domain + app + auth + monitor）
    rca-infra/                  # 基础设施（api + storage + notify + update-check）
    rca-shared/                 # 可选：跨层共享类型（仅在必要时引入）
```

### 5.1 各 crate 职责

- `rca-core`
  - 聚合领域模型、登录状态机、监控编排、应用用例层
  - 对外暴露统一 `AppService` 与 `CoreEvent`
  - 通过 trait 依赖 `rca-infra` 提供的能力（协议/存储/通知）

- `rca-infra`
  - 封装 HTTP/WS 协议客户端、配置与会话存储、keyring、系统通知、更新检查
  - 为 `rca-core` 提供稳定的端口实现（adapters）
  - 隔离平台差异与第三方库波动

- `rca-shared`（可选）
  - 承载确实跨层复用且不含业务行为的通用类型（如分页、时间范围、公共错误码）
  - 仅当 `rca-core` 与 `rca-infra` 出现双向重复定义风险时引入

- `apps/rca-desktop`
  - Slint 页面、ViewModel、UI 命令绑定
  - 仅依赖 `rca-core`（通过构造注入 `rca-infra` 适配器）

### 5.2 旧拆分到新拆分映射

- `rca-domain` + `rca-auth` + `rca-monitor` + `rca-app` → `rca-core`
- `rca-api` + `rca-storage` + `rca-notify` + `update-check` → `rca-infra`
- `rca-observability` → 先并入 `rca-core`（若后续复用强再独立）

### 5.3 依赖方向规则

1. 上层依赖下层，禁止反向依赖。
2. `rca-core` 通过 trait 依赖抽象端口，不直接绑定具体基础设施实现。
3. `apps/rca-desktop` 仅依赖 `rca-core` 的应用接口与视图 DTO。
4. 跨 crate 通信优先 trait + DTO，避免共享可变全局状态。

---

## 6. 数据流与事件流

## 6.1 主链路（登录到自动处理）

1. UI 发起登录命令
2. `rca-core` 调用认证用例，`rca-infra` 执行扫码协议并写入会话
3. `rca-core` 启动课程轮询与 WS 监听（底层由 `rca-infra` 提供协议流）
4. 新课程/新题目/签到窗口触发 `CoreEvent`
5. `rca-core` 根据策略调用签到/答题接口
6. `rca-infra` 推送系统通知
7. UI 订阅事件流并更新状态

## 6.2 事件模型

事件分层：

- 领域事件：`LessonStarted`、`ProblemPublished`、`CheckinOpened`
- 应用事件：`AutoAnswerSubmitted`、`CheckinSucceeded`
- UI 事件：`LoginRequested`、`StartMonitorRequested`

事件总线要求：

- 支持背压与丢弃策略配置
- 对关键事件至少一次投递（At-Least-Once）
- 事件带关联 ID（trace_id / lesson_id / request_id）

---

## 7. 跨平台能力抽象

### 7.1 通知

- 统一 `Notifier` trait
- Linux 优先 `notify-rust`
- Windows/macOS 后端通过 feature 或运行时探测选择
- 不可用时降级为日志 + UI 内提示

### 7.2 凭据存储

- 统一 `CredentialStore` trait
- 默认后端：`keyring`
- 不可用时明确错误，不回退到明文

### 7.3 更新检查

- 统一 `UpdateChecker` trait
- 默认来源：GitHub Releases API（或固定发布元数据）
- 行为：通知可用版本 + 打开下载页

---

## 8. 可靠性与并发策略

### 8.1 生命周期管理

- 所有后台任务由 `TaskSupervisor` 管理
- 启停顺序可控：登录成功后启动监控；退出时先停监控再关闭 UI
- 提供优雅退出与超时强制终止机制

### 8.2 重试策略

- 网络调用采用指数退避 + 抖动
- 明确不可重试错误（认证失败、权限问题）
- 重连与重试事件写入观测系统

### 8.3 一致性策略

- 会话更新为原子写（临时文件 + rename）
- UI 状态更新通过单入口 reducer，避免竞态

---

## 9. 安全与隐私

1. token 仅通过 keyring 存储，不写入普通配置文件。
2. 日志默认脱敏（手机号、token、用户标识）。
3. 配置文件最小化存储，避免保留敏感业务内容。
4. 对外网络请求统一设置超时、TLS 校验与用户代理标识。

---

## 10. 可观测性与诊断

- `tracing` 统一日志与 Span
- 关键路径埋点：登录耗时、API 成功率、重试次数、事件延迟
- 本地诊断包：最近 N 条结构化日志 + 运行环境摘要（非敏感）

---

## 11. UI 顶层设计（Slint）

### 11.1 信息架构

- 总览：登录状态、当前课程、系统状态
- 课程：进行中/近期课程与事件流
- 设置：账号、通知、行为策略、更新检查
- 日志：可筛选事件与错误摘要

### 11.2 状态管理

- 单向数据流：`UiCommand -> AppUseCase -> CoreEvent -> ViewState`
- UI 不直接操作底层服务对象
- 所有异步结果经应用层归一化后再映射至 ViewState

### 11.3 视觉与交互

- 扁平化组件与统一 token（间距、圆角、字体层级）
- 明确成功/警告/错误语义色
- 键盘可达性、焦点可见、屏幕缩放适配

---

## 12. 质量门禁与工程规范

### 12.1 代码质量基线

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- 必要场景加入集成测试（fake notifier / temp fs）

### 12.2 测试策略

- 单元测试：领域规则与状态机
- 集成测试：登录、监控、签到/答题流程
- 契约测试：接口 JSON 字段兼容性
- 冒烟测试：三平台启动与关键链路

### 12.3 文档策略

- API 变更需同步更新详细设计文档
- 非向后兼容改动必须记录迁移说明

---

## 13. 迁移路线（建议）

### Phase 1（MVP 核心）

- 建立 workspace 与 3~4 crate 结构
- 打通登录 -> 监控 -> 签到/答题 -> 通知
- 完成 Slint 基础页面与状态流

### Phase 2（可用性增强）

- 完善错误恢复、重试、诊断
- 增强策略配置与日志过滤
- 三平台细节适配收敛

### Phase 3（功能扩展）

- 评估并恢复高级功能（如 PPT 相关）
- 增加更多通知通道/插件机制

---

## 14. 主要风险与应对

1. **协议变更风险**：通过 `rca-infra` 协议适配层的契约测试与版本探测隔离影响。
2. **三平台行为差异**：能力探测 + 降级路径 + 平台冒烟测试。
3. **并发复杂度增长**：统一任务监管与严格状态机边界。
4. **UI 与业务耦合回潮**：审查规则限制 UI 层直接网络访问。

---

## 15. 决策记录（ADR 级）

- ADR-001：采用 Cargo workspace + 3~4 crate 分层（避免过度拆分）
- ADR-002：UI 与核心逻辑严格解耦
- ADR-003：敏感凭据仅允许 keyring 存储
- ADR-004：更新策略采用“检查 + 跳转下载”
- ADR-005：MVP 以核心自动化链路为先，不绑定高风险扩展能力
