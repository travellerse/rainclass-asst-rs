# rca-desktop - Desktop GUI Application

**Purpose:** Slint-based desktop GUI for RainClassroom Assistant.

## STRUCTURE

```
rca-desktop/
├── src/
│   ├── main.rs           # Entry point, i18n setup
│   ├── app_controller.rs # DesktopController, app lifecycle
│   ├── ui_binding.rs     # Slint UI event bindings
│   ├── event_subscriber.rs # Background event loop
│   ├── login_flow.rs     # QR login UI flow
│   ├── command_runner.rs # Async command execution
│   ├── config_mapping.rs # Config <-> UI mapping
│   └── ui_helpers.rs     # UI utilities
└── ui/                   # .slint UI files
    ├── components/
    └── pages/
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add UI page | `ui/pages/` | Slint component |
| Bind UI events | `ui_binding.rs` | Connect Slint callbacks |
| Handle login | `login_flow.rs` | QR display, polling |
| Background tasks | `event_subscriber.rs` | Event loop, notifications |
| App lifecycle | `app_controller.rs` | Bootstrap, shutdown |

## CONVENTIONS

- `DesktopController` wraps `AppServiceImpl` with UI-specific logic
- Global `I18n` callback for translations: `ui.global::<I18n>().on_t(...)`
- `windows_subsystem = "windows"` in release (no console window)
- UI weak references for async callbacks to avoid leaks

## ANTI-PATTERNS

- **DON'T** block UI thread - use `tokio::spawn` for async ops
- **DON'T** forget weak refs in closures passed to Slint
