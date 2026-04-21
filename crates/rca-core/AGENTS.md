# rca-core - Domain Layer

**Purpose:** Core business logic, domain models, CQRS application services, and auth state machine.

## STRUCTURE

```
rca-core/src/
├── app/           # CQRS: commands, queries, service impl
├── auth/          # Auth state machine and service
├── domain/        # Domain models (lesson, checkin, events, etc.)
├── monitor/       # Lesson monitoring engine
└── utils/         # Logger utilities
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add command | `app/command.rs` | Extend `AppCommand` enum |
| Add query | `app/query.rs` | Extend `AppQuery` enum |
| Modify auth flow | `auth/state_machine.rs` | Auth state transitions |
| Add domain event | `domain/events.rs` | Lesson lifecycle events |
| Add monitor logic | `monitor/engine.rs` | WebSocket event handling |
| Define port | `app/ports.rs` | Repository/adapter interfaces |

## CONVENTIONS

- CQRS: Separate `AppCommand` (write) and `AppQuery` (read)
- `AppServiceImpl` handles both commands and queries
- Auth state machine uses explicit state transitions
- Domain errors in `domain/errors.rs`

## ANTI-PATTERNS

- **DON'T** bypass `AppService` - all domain ops go through it
- **DON'T** mix sync/async in domain logic - use `async-trait` for ports
