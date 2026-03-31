---
name: slint-rust-app-dev
description: Use when 用户要用 Rust 开发 Slint 应用（新建项目、UI/状态架构、Slint↔Rust 绑定、列表/模型、线程与异步、性能与打包），并希望按 Slint 文档与最佳实践产出可运行代码。
---

# Slint (Rust) 应用开发 Skill

## Goal

把用户的产品需求快速落地为**可编译、可运行、可迭代**的 Slint + Rust 应用代码：从项目脚手架、UI 结构、数据模型、事件回调，到线程/异步、性能优化与交付打包。

本 skill 优先遵循 Slint 的设计理念：
- UI 用 `.slint` 进行**声明式**描述（布局、样式、绑定）。
- 业务逻辑与数据处理放在 **Rust**。
- UI 与 Rust 通过 **properties / callbacks / models** 进行清晰边界的连接。

## Version Target（适配版本）

本 skill 以 **Slint v1.15.1**（Rust crate `slint = "1.15.1"`）为默认目标版本：
- 需要升级到 1.15.1：Rust 侧通常只需 `cargo update`（并检查锁文件变更）。
- 需要迁移旧 `.slint` 语法：优先使用 `slint-updater`（语法感知的迁移工具）批量升级 UI 文件，再做手工回归。

## When to Use (Triggers)

当用户提出以下类型请求时触发：
- “用 Slint + Rust 做一个桌面应用/GUI”
- “把这个界面用 Slint 写出来，并接上 Rust 逻辑”
- “Slint 的列表/表格怎么绑定数据？”
- “Slint 回调、属性绑定、双向绑定怎么设计？”
- “Slint 应用怎么做多线程/异步/定时器/后台任务？”
- “Slint 工程怎么组织、怎么打包发布？”

## Intake（最小信息）

在开始编码前，收集**足够**的信息即可；未提供时按默认值继续并在输出中声明假设。

1) 目标平台与交付形式  
   - 桌面（Windows/macOS/Linux）/ 嵌入式 / 其他  
   - 单可执行文件 / 安装包 / 内部工具

2) UI 范围  
   - 主要页面/组件清单（例如：主窗口、侧边栏、表单、列表、详情、设置）
   - 交互流程（最少 3-5 条关键用户路径）

3) 数据与状态  
   - 数据来源：本地文件 / 内存 / HTTP API / 数据库
   - 列表数据规模（几十/几千/更大）
   - 是否需要搜索/排序/分页

4) 非功能需求  
   - 性能目标（启动时间、滚动流畅度）
   - 主题/暗色模式/国际化
   - 日志、错误提示、可观测性

## Workflow（执行步骤）

### 1) 设定架构边界（先设计，再写 UI）

输出一段简短的架构说明（写在 README 或注释顶部即可）：
- `.slint`：只做**展示与交互**（绑定、布局、动画、样式）
- `src/`：状态/领域逻辑/服务层（网络、存储、计算）
- UI↔Rust 通道：
  - UI 读：Rust 设置 property（或 model）→ UI 自动更新
  - UI 写：UI callback → Rust 处理 → 再更新 property/model

避免把业务流程写进 `.slint` 的 imperative 逻辑；`.slint` 中的逻辑应尽量保持轻量（简单转换、显示控制）。

### 2) 初始化工程（Rust + Slint 常用结构）

推荐结构（可按项目规模调整）：
- `ui/`：`.slint` 文件（按页面/组件拆分）
- `src/main.rs`：启动、依赖装配、窗口创建
- `src/app/`：应用状态与控制器（例如 `AppState`, `AppController`）
- `src/services/`：网络/持久化/系统集成
- `src/models/`：领域模型与 UI 模型桥接（列表 Model、DTO）
- `build.rs`：使用 `slint-build` 编译/嵌入 `.slint`（当你选择 build-time include 方案）

在输出代码时：
- 明确写出 `Cargo.toml` 的关键依赖
- 若需要 `build.rs`，把它一并给出

### 3) 设计 UI 组件与数据绑定

#### 3.1 Property 命名与类型

约定：
- `property <type> xxx`：UI 可读（由 Rust 设置，UI 绑定展示）
- `in property ...` / `out property ...` / `in-out property ...`：按读写需求设定方向（优先最小权限）
- 类型选择：能用简单类型（string/int/bool/float）就不用复杂结构；复杂结构优先通过 model 或序列化边界处理

补充（Slint 1.15+）：当 UI 表单需要双向编辑复杂数据时，优先考虑：
- 定义 `struct` 类型并通过 `in-out property <Struct>` 暴露
- 使用 `field <=> data.field` 的结构体字段双向绑定（避免把每个字段拆成一堆离散 property，降低样板代码）

#### 3.2 Callback 设计

约定：
- callback 名称表达**意图**：`request_save()`, `request_delete(id)`, `request_refresh()`
- callback 只传**必要参数**，避免把大对象从 UI 传回 Rust（可传 id / index / key）
- Rust 侧处理失败时，通过一个 `error_message` property 或 `toast`/`dialog` 状态回写到 UI

#### 3.3 List / Table：用 Model 连接大量数据

当需要展示列表/表格或频繁增删改：
- 使用 Slint 的 Model 体系（在 Rust 侧构造 model，传给 UI）
- 更新策略优先选择**增量更新**（替换整表会导致性能抖动）
- 数据结构上，UI item 用“渲染所需字段”的轻量结构，领域对象保留在 Rust 层（必要时用映射/缓存）

#### 3.4 动态布局：优先利用 GridLayout 的数据驱动能力（Slint 1.15+）

当 UI 需要“表格/宫格/动作面板/动态行列”：
- 优先使用 `GridLayout` + `for`/`if` 构造数据驱动网格，而不是在 Rust 里手工拼接大量组件实例
- `row/col/rowspan/colspan` 允许绑定与运行时变化时，注意配合模型更新与空态/加载态，保持布局稳定

### 4) 线程与异步（保持 UI 线程轻量）

原则：
- UI 线程只做渲染与轻计算；耗时任务（IO、网络、解析、加密、图片处理）放到后台线程/异步任务
- 从后台更新 UI 时，使用 Slint 提供的安全调度方式（不要直接跨线程触碰 UI 对象）：
  - 跨线程：用 `slint::invoke_from_event_loop(...)` 把“更新 UI”的闭包丢回事件循环线程执行
  - UI 线程内 async：用 `slint::spawn_local(async { ... })` 在 Slint 事件循环中执行 future（通常在 UI callback 中调用）
  - 需要捕获 UI handle：优先用 `handle.as_weak()`，并在事件循环里 `upgrade()` 再更新，避免生命周期/所有权问题

实践建议：
- 将“触发后台任务”封装在 `AppController`：UI callback → controller → spawn task → 结果回写 state → 更新 UI properties/models
- 对用户可见的后台状态：`is_loading`、`progress`、`status_text` properties

异步运行时兼容性提醒（Slint 1.15.1 文档要点）：
- `spawn_local` 运行在平台相关的 UI 事件循环上；不要假设它能驱动任意 async runtime
- Tokio 需要额外注意（公平性/调度/进入 runtime 上下文等）；默认不建议 `#[tokio::main]`
- 若项目必须用 Tokio：按文档建议用 `async_compat::Compat::new(...)` 或将进入 Slint 事件循环的调用包进 `tokio::task::block_in_place(...)`

### 5) 资源、主题与可维护性

- 图片/字体等资源：建立统一 `assets/`（或 crate resource）策略，并在 README 说明
- 主题：把颜色/间距/字体集中为少量可配置 tokens，避免散落魔法数
- 组件拆分：超过 ~200 行的 `.slint` 文件应拆成组件与子组件

### 6) 性能与质量收尾

性能检查清单：
- 列表：是否避免全量替换？是否避免每帧重建大量对象？
- 绑定：是否存在昂贵表达式在高频更新中重复计算？
- 图片：是否过大导致 GPU/CPU 压力？是否需要缩放/缓存？

质量检查清单：
- `cargo fmt`、`cargo clippy` 无明显问题
- 错误路径完善：网络失败、空数据、权限不足、文件不存在
- UI 可用性：禁用态、加载态、空态、错误态齐全

## Output Contract（你必须产出什么）

当用户请求“做一个 Slint 应用/实现某功能”时，交付应至少包含：
1) 可运行的 Rust 工程（含 `Cargo.toml`、`src/`、`.slint` 文件与必要的 `build.rs`/宏 include）
2) 关键 UI 绑定点说明（properties、callbacks、models 的表格或列表）
3) 运行方式（`cargo run` / feature flags / 平台注意事项）

当用户只问“怎么做/最佳实践/架构建议”时，交付应至少包含：
- 推荐结构 + 最小示例代码片段（UI + Rust 两边）
- 常见坑与规避方式（线程、model 更新、资源路径）

## Resource Use（本 skill 自带参考）

如 workspace 中存在本 skill 的 `references/` 文件：
- 先阅读 `references/slint-rust-best-practices.md` 获取约定与模板片段
- 需要做决策时优先按参考中的默认值（例如：回调命名、状态字段、列表 model 更新方式）

## Quality Bar（质量标准 / 禁止项）

必须满足：
- 输出代码**可编译**（依赖/feature/宏/路径自洽）
- UI/Rust 边界清晰：UI 不承载复杂业务流程
- 列表使用 model 思维；不要把大量数据直接塞进一堆独立 properties
- 线程安全：不跨线程直接操作 UI 对象

尽量避免：
- 把状态散落在多个全局变量中（集中在 `AppState` 或 controller）
- UI callback 里做 IO 或长计算
- 为了“方便”在 `.slint` 里写大量 imperative 逻辑

## Default Assumptions（若用户没说明）

若用户未指明，默认：
- Rust stable + Cargo
- Slint 版本：**1.15.1**
- Rust 工具链最低要求：**MSRV 1.88**（若用户的 toolchain 更旧，应先升级 Rust 或改用更旧的 Slint 版本）
- 桌面应用（winit/默认后端），单窗口为主（需要多窗口再扩展）
- 项目规模：中小型（可拆分 UI 与 controller）
- 不使用 unsafe；不引入重型依赖除非必要

