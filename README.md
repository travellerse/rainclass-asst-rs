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

## 运行模式

`rca-cli` 与 `rca-desktop` 的 API 默认策略一致：

- 所有构建默认 `真实 API`

## 快速开始

1. 构建：

    ```bash
    cargo build --workspace
    ```

2. 运行桌面端（默认真实 API）：

    ```bash
    cargo run -p rca-desktop
    ```

3. 运行桌面端（Release，默认真实 API）：

    ```bash
    cargo run -p rca-desktop --release
    ```

4. 运行 CLI：

    ```bash
    cargo run -p rca-cli -- status
    ```

## CLI 命令

- 查看状态：`cargo run -p rca-cli -- status`
- 发起扫码登录并轮询：`cargo run -p rca-cli -- login --attempts 20 --interval-secs 1`

    > CLI 会在终端直接渲染二维码（基于 ticket URL），可直接微信扫码。
- 刷新会话：`cargo run -p rca-cli -- refresh-session`
- 启动监控（后台任务）：`cargo run -p rca-cli -- start-monitor`
- 监控并持续打印事件：`cargo run -p rca-cli -- monitor --duration-secs 60`
- 停止监控：`cargo run -p rca-cli -- stop-monitor`
- 检查更新：`cargo run -p rca-cli -- check-update`
- 查看近期事件：`cargo run -p rca-cli -- events --limit 20`
- 登出：`cargo run -p rca-cli -- logout`

## 文档

- 架构总览：[docs/01-top-level-architecture.md](docs/01-top-level-architecture.md)
- 详细设计：[docs/02-detailed-structs-and-signatures.md](docs/02-detailed-structs-and-signatures.md)
