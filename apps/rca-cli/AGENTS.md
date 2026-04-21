# rca-cli - Command Line Interface

**Purpose:** CLI for RainClassroom automation (status, login, monitor, download).

## STRUCTURE

```
rca-cli/src/
└── main.rs    # CLI commands, QR rendering, async main
```

## COMMANDS

```bash
cargo xtask run-cli -- status              # Show auth/monitor state
cargo xtask run-cli -- login               # QR login flow
cargo xtask run-cli -- refresh-session     # Validate/refresh token
cargo xtask run-cli -- monitor             # Monitor lessons (blocking)
cargo xtask run-cli -- events --limit 20   # Show recent events
cargo xtask run-cli -- download-ppt --presentation-id 123
cargo xtask run-cli -- lessons             # List active lessons
cargo xtask run-cli -- logout              # Clear session
cargo xtask run-cli -- check-update        # Check for updates
cargo xtask run-cli -- config get          # Show config
cargo xtask run-cli -- config set-tenant Rain
```

## CONVENTIONS

- Uses `clap` derive macros for CLI definition
- QR codes rendered in terminal using `qrcode` crate
- Spinner with `indicatif` for long operations
- Auto-download PPT with `--auto-download-ppt` flag

## ANTI-PATTERNS

- **DON'T** use `println!` for user-facing output - use `tracing::info!`
- **DON'T** forget to call `ensure_logged_in()` before protected commands
