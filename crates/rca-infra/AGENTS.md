# rca-infra - Infrastructure Layer

**Purpose:** API adapters, storage implementations, notifications, and update checking.

## STRUCTURE

```
rca-infra/src/
├── api/           # HTTP/WebSocket client, DTOs, port impl
├── bridge/        # Adapters between infra and core ports
├── log/           # Tracing/logger setup
├── notify/        # Desktop/webhook/multi notifier impls
├── storage/       # Config, session, credential storage
├── tenant/        # Tenant/host resolution
└── update/        # GitHub release checker
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Modify API calls | `api/core_port/` | HTTP, WS, QR login, download |
| Add storage backend | `storage/` | Implement `ConfigRepository` trait |
| Add notifier | `notify/` | Implement `Notifier` trait |
| Bridge to core | `bridge.rs` | Adapter implementations |
| Update checking | `update/checker.rs` | GitHub releases API |

## CONVENTIONS

- Port implementations live in `api/core_port/`
- Storage: JSON files for config/session, keyring for credentials
- Notifiers: `LoggingNotifier` (CLI), `DesktopNotifier` (GUI), `MultiNotifier` (combined)
- Errors per module: `api/errors.rs`, `storage/errors.rs`, etc.

## ANTI-PATTERNS

- **DON'T** call core directly - use bridge adapters
- **DON'T** expose infra types in core - map to domain types
