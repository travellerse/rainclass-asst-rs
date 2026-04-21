# RainClassroom Assistant - Project Knowledge Base

**Generated:** 2026-04-21
**Commit:** 5aec6fe
**Branch:** dev

## OVERVIEW

Rust-based 雨课堂 (RainClassroom) assistant with layered architecture. Provides CLI and desktop GUI for classroom automation including QR login, lesson monitoring, PPT download, and notifications.

## STRUCTURE

```
rainclass-asst-rs/
├── apps/
│   ├── rca-cli/         # CLI binary (clap-based)
│   └── rca-desktop/     # Slint GUI binary
├── crates/
│   ├── rca-app/         # Bootstrap layer (init_logger, bootstrap_core_app)
│   ├── rca-core/        # Domain logic (CQRS, auth state machine, monitor engine)
│   └── rca-infra/       # Infrastructure (API adapters, storage, notifications)
├── xtask/               # Build task runner (cargo xtask <cmd>)
├── locales/             # i18n translations (zh-CN fallback)
└── webdata/             # Web scraping data
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add CLI command | `apps/rca-cli/src/main.rs` | Subcommands in `Command` enum |
| Modify desktop UI | `apps/rca-desktop/src/` | `main.rs`, `ui_binding.rs`, `app_controller.rs` |
| Core business logic | `crates/rca-core/src/` | `app/`, `auth/`, `domain/`, `monitor/` |
| API/HTTP layer | `crates/rca-infra/src/api/` | `core_port/`, `client.rs`, `dto.rs` |
| Storage impl | `crates/rca-infra/src/storage/` | Config, session, credentials |
| Notifications | `crates/rca-infra/src/notify/` | Desktop, webhook, multi-notifier |
| Add build task | `xtask/src/main.rs` | Extend `Task` enum, add handler |
| i18n strings | `locales/` | `zh-CN/` fallback, add new keys to all |

## CONVENTIONS

### Rust Edition
- Edition 2024 (workspace-wide)
- Async/await with tokio
- `thiserror` for error types
- `tracing` for logging

### Dependency Management
- Workspace-level deps in root `Cargo.toml`
- Crate-specific deps use `workspace = true`
- `cargo-deny` for license/advisory checking (see `deny.toml`)

### Code Organization
- CQRS pattern in `rca-core/src/app/`: `AppCommand`, `AppQuery`, `AppService`
- Port-adapter pattern: `rca-core` defines ports, `rca-infra` implements
- State machine for auth: `rca-core/src/auth/state_machine.rs`

### Testing
- Unit tests in `#[cfg(test)]` modules
- Integration tests in `crates/rca-infra/tests/`
- Desktop tests are lenient in CI (`--lenient-desktop`)

## ANTI-PATTERNS

- **DON'T** use `std::sync::Mutex` across await points - use `tokio::sync::Mutex`
- **DON'T** call blocking operations in async without `spawn_blocking`
- **DON'T** modify `Cargo.lock` manually - use `cargo update -p <pkg>`
- **DON'T** add deps without checking `deny.toml` license compatibility
- **DON'T** bypass `xtask` for common tasks - always `cargo xtask <cmd>`

## UNIQUE STYLES

### xtask Pattern
All build/dev commands go through `xtask`:
```bash
cargo xtask lint          # fmt + clippy
cargo xtask test          # workspace tests
cargo xtask run-desktop   # run GUI
cargo xtask run-cli -- status  # run CLI with args
```

### Bootstrap Flow
Both apps delegate to `rca_app::bootstrap_core_app()`:
1. Detect paths (`AppPaths`)
2. Build infrastructure adapters
3. Inject into `AppServiceImpl`
4. Run startup actions (load config, restore session)

### i18n
- `rust_i18n` macro: `i18n!("../../locales", fallback = "zh-CN")`
- Use `t!("key")` for translations
- Keys prefixed by context: `cli_`, `desktop_`, `error_`

### Slint UI
- `.slint` files in `apps/rca-desktop/ui/`
- `slint::include_modules!()` in `main.rs`
- Global `I18n` for translation callbacks

## COMMANDS

```bash
# Development
cargo xtask fmt              # Format all
cargo xtask clippy           # Lint
cargo xtask lint             # Both fmt + clippy
cargo xtask test             # Run tests
cargo xtask build            # Debug build
cargo xtask build --release  # Release build

# Running
cargo xtask run-desktop              # GUI mode
cargo xtask run-desktop --release    # Optimized GUI
cargo xtask run-cli -- status        # CLI status
cargo xtask run-cli -- login         # QR login flow
cargo xtask run-cli -- monitor       # Monitor lessons

# CI/Release
cargo xtask lint --check     # CI lint mode
cargo xtask test --locked --lenient-desktop  # CI test mode
cargo xtask release --locked # CI release build
```

## NOTES

- **Desktop binary**: Uses `windows_subsystem = "windows"` in release (no console)
- **Slint GUI**: Requires `libfontconfig-dev` on Linux (see CI)
- **QR Login**: CLI renders QR in terminal using `qrcode` crate
- **Session Storage**: Keyring for credentials, JSON files for session/config
- **WebSocket**: Real-time lesson monitoring via `tokio-tungstenite`
- **PDF Export**: Downloads PPT as images, assembles with `pdf-writer`

## CRATE DEPENDENCIES

```
rca-cli → rca-app, rca-core, rca-infra
rca-desktop → rca-app, rca-core
rca-app → rca-core, rca-infra
rca-infra → rca-core
rca-core → (no internal deps)
```
