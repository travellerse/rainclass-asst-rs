# RainClassroom Assistant (Rust)

Rust 重构版本的雨课堂助手，采用分层架构：

- `rca-core`：应用服务、领域模型、端口定义
- `rca-infra`：API 适配、存储、更新、通知实现
- `rca-cli`：命令行入口（无 GUI）
- `rca-desktop`：Slint 桌面端

## 当前目标与阶段

本项目按阶段推进：

- Phase 1（MVP）：登录、课程监控、自动签到、自动答题、更新检查
- Phase 2（增强）：弹幕跟随、PPT 下载与题目提取、语音提醒、更多 UI 配置

## Desktop UI 分层

`rca-desktop` 的 Slint UI 采用组件化分层：

- `apps/rca-desktop/ui/app-window.slint`：主窗口壳层（侧边栏导航 + 页面路由 + 回调透传）
- `apps/rca-desktop/ui/pages/`：页面层（`overview-page.slint`、`events-page.slint`、`settings-page.slint`）
- `apps/rca-desktop/ui/components/`：通用组件层（`glass-card.slint`、`toggle-switch.slint`、`primary-button.slint`、`nav-item.slint`）
- `apps/rca-desktop/ui/models.slint`：UI 共享数据结构（课程与事件）
- `apps/rca-desktop/ui/theme.slint`：主题 token（颜色、阴影、动效时长、间距、字号）

该分层遵循“页面负责布局，组件负责视觉与交互细节，主窗口只做编排”的原则，方便后续继续扩展视觉风格与业务状态。

## 快速开始

1. 开发检查：

    ```bash
    cargo xtask lint
    ```

2. 运行测试：

    ```bash
    cargo xtask test
    ```

3. 构建：

    ```bash
    cargo xtask build
    ```

4. 运行桌面端：

    ```bash
    cargo xtask run-desktop
    ```

5. 运行桌面端（Release）：

    ```bash
    cargo xtask run-desktop --release
    ```

6. 运行 CLI：

    ```bash
    cargo xtask run-cli -- status
    ```

## xtask 命令

- `cargo xtask fmt`：执行 `cargo fmt --all`
- `cargo xtask fmt --check`：执行只读格式检查，适合 CI
- `cargo xtask clippy`：执行 `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo xtask lint`：执行 `cargo fmt --all` 和 `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo xtask lint --check`：依次执行格式检查和 clippy
- `cargo xtask test`：执行 `cargo test --workspace --all-features`
- `cargo xtask test --locked --lenient-desktop`：CI 兼容模式，允许桌面端测试失败但不阻塞流水线
- `cargo xtask build`：构建整个 workspace
- `cargo xtask build --release`：以 release 配置构建整个 workspace
- `cargo xtask release`：release 构建辅助入口
- `cargo xtask run-desktop [--release] [-- <args...>]`：运行桌面端
- `cargo xtask run-cli -- <args...>`：运行 CLI 并透传子命令参数
- `cargo xtask coverage`：执行 `cargo llvm-cov --workspace --all-features --html`，需先安装 `cargo-llvm-cov`

## 原始 Cargo 入口

如果需要，也可以继续直接使用 Cargo 原生命令：

- 构建 workspace：`cargo build --workspace`
- 运行桌面端：`cargo run -p rca-desktop`
- 运行桌面端（Release）：`cargo run -p rca-desktop --release`
- 运行 CLI：`cargo run -p rca-cli -- status`

## CLI 命令

使用 xtask 运行 CLI：

    ```bash
    cargo xtask run-cli -- status
    ```

- 查看状态：`cargo xtask run-cli -- status`
- 发起扫码登录并轮询：`cargo xtask run-cli -- login --attempts 20 --interval-secs 1`

    > CLI 会在终端直接渲染二维码（基于 ticket URL），可直接微信扫码。
- 刷新会话：`cargo xtask run-cli -- refresh-session`
- 启动监控（后台任务）：`cargo xtask run-cli -- start-monitor`
- 监控并持续打印事件：`cargo xtask run-cli -- monitor --duration-secs 60`
- 停止监控：`cargo xtask run-cli -- stop-monitor`
- 检查更新：`cargo xtask run-cli -- check-update`
- 查看近期事件：`cargo xtask run-cli -- events --limit 20`
- 登出：`cargo xtask run-cli -- logout`

## 文档

- 架构总览：[docs/01-top-level-architecture.md](docs/01-top-level-architecture.md)
- 详细设计：[docs/02-detailed-structs-and-signatures.md](docs/02-detailed-structs-and-signatures.md)
