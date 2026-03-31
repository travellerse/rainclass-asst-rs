---
title: Slint + Rust 最佳实践速查
purpose: 给 skill 提供可复用的约定、模板与常见坑规避。
---

# Slint + Rust 最佳实践速查

> 这份参考面向“写得快且可维护”的默认路线：UI 声明式、逻辑 Rust 化、清晰边界、可增量迭代。

## 0. 版本与升级（面向 Slint 1.15.1）

- Rust 项目建议显式 pin：`slint = "1.15.1"`（应用层使用 `slint` crate；不要直接依赖 `i-slint-*` 内部 crate）。
- 升级 patch 版本：通常 `cargo update` 即可，但要检查 lockfile 并做基本回归。
- `.slint` 语法迁移：优先用 `slint-updater`（语法感知迁移工具）批量升级，再手工修边角。
- 渲染后端与依赖：1.15 系列已升级到 WGPU 28；若项目显式使用 wgpu 相关 API/feature，升级时需关注依赖联动与 feature 选择。

## 1. UI↔Rust 接口设计（推荐约定）

### 1.1 Properties：状态向 UI 单向流动

推荐 UI 侧定义：
- `in property <type> ...`：Rust → UI（只读）
- `in-out property <type> ...`：需要 UI 编辑时才开放（例如输入框绑定）

命名建议：
- 以“UI 语义”命名：`status_text`, `is_loading`, `error_message`, `search_query`
- 避免“后端字段名直出”污染 UI（例如 `user_name` 还好；`usr_nm` 不要）

### 1.2 Callbacks：用户意图从 UI 传回 Rust

推荐形式：
- `callback request_refresh();`
- `callback request_open(id: int);`
- `callback request_save();`

原则：
- callback 传“最小必要参数”（id/index/key），不要把大对象从 UI 传回 Rust
- Rust 处理失败时，通过 `error_message` / `toast_text` / `dialog_state` 回写 UI

## 2. 列表与大数据：Model 思维

适用场景：
- ListView/TableView/Repeater 等
- 需要频繁更新、排序、过滤

推荐做法：
- Rust 侧维护“领域数据”（完整对象）
- Rust 侧另外维护“UI 渲染数据”（轻量行结构），通过 Slint Model 传给 UI
- 更新尽量走增量：插入/删除/修改对应行，而不是每次全量替换

常见坑：
- 每次更新都替换整个 model → 滚动抖动、CPU 飙升
- UI 绑定表达式过重且在高频变化时重复计算 → 掉帧

## 3. 线程与异步：UI 线程永远轻量

原则：
- IO/网络/解析/压缩/图片处理都放后台
- UI 线程只做：渲染、少量状态切换、触发任务

推荐流程：
1) UI callback → Rust controller
2) controller 启动后台任务（线程或 async runtime）
3) 任务完成后：调度回 UI 线程更新 properties/models

### 3.1 跨线程更新 UI：invoke_from_event_loop

当后台线程算完结果，需要更新 UI：
- 使用 `slint::invoke_from_event_loop(move || { /* set properties */ })`
- 传入闭包必须是 `Send + 'static`
- UI handle 优先用 `as_weak()` 传递到线程里，回到事件循环再 `upgrade()` 更新

### 3.2 UI 事件循环内跑 Future：spawn_local

当你要在 UI callback 中执行 async（但不想阻塞 UI）：
- 使用 `slint::spawn_local(async move { ... })`
- 仅在启动 Slint 事件循环的主线程调用
- 从其他线程想跑 `spawn_local`：先 `invoke_from_event_loop` 丢回主线程再调用 `spawn_local`

### 3.3 Tokio 兼容性（常见坑）

在 Slint 事件循环里直接“等待 Tokio future”可能不工作：
- 默认不建议 `#[tokio::main]`
- 必须用 Tokio 时：按官方建议用 `async_compat::Compat::new(...)` 包裹传给 `spawn_local` 的 future，或将进入 Slint 事件循环的调用包进 `tokio::task::block_in_place(...)`

常见坑：
- 在 callback 里直接同步请求网络/读文件 → UI 卡死
- 后台线程直接拿着 UI handle 做更新 → 线程安全问题

## 4. 数据绑定增强（Slint 1.15+）

### 4.1 struct 字段双向绑定

表单类 UI：优先用 `in-out property <YourStruct>` 承载状态，然后对字段做 `<=>` 双向绑定，减少样板与错误率。

### 4.2 动态 GridLayout

当 UI 是“网格/表格/动作面板”且数据驱动：
- `GridLayout` 支持 `for`/`if` 生成行/格子
- `row/col/rowspan/colspan` 可绑定与运行时变化：配合 model 更新，避免 UI 抖动与重排风暴

## 5. 工程组织：可扩展但不臃肿

推荐目录（中小型）：
- `ui/`：`.slint`（按页面/组件拆）
- `src/app/`：状态与 controller
- `src/services/`：网络/存储/系统
- `src/models/`：领域模型与 UI bridge

拆分信号：
- `.slint` 单文件 >200 行且难以定位：拆子组件
- `main.rs` 变成“胶水巨兽”：引入 `AppController`

## 6. 可用性与边界状态（必须补齐）

每个核心页面至少考虑：
- 加载态：`is_loading`
- 空态：`is_empty` 或 `items_count == 0`
- 错误态：`error_message` + 明确重试入口
- 禁用态：保存按钮在表单无效时禁用

## 7. 性能速查

- 列表：增量更新优先；尽量减少每帧创建对象
- 图片：避免超大原图直接渲染；必要时缩放/缓存
- 绑定表达式：将昂贵计算放 Rust，UI 只绑定结果

## 8. 输出模板（给 assistant 的“交付结构”）

当用户要一个功能，输出尽量按下面顺序：
1) “我将如何组织 UI 与 Rust”（3-6 行）
2) `ui/*.slint` 关键片段（properties/callbacks/model）
3) `src/` 关键片段（controller/state/service）
4) 运行/调试命令与注意事项

